const fs = require('fs');
const { chromium } = require('playwright-core');
const qrcode = require('qrcode-terminal');

const CDP_WS = 'ws://127.0.0.1:9222/devtools/browser';
const COOKIE_FILE = './cookies.json';
const POLL_INTERVAL_MS = 2000;
const MAX_WAIT_MS = 3 * 60 * 1000;

const STATUS = {
  0: 'success',
  86038: 'qrcode expired',
  86090: 'scanned, awaiting confirm in app',
  86101: 'waiting for scan',
};

// Obscura's Runtime.evaluate ignores awaitPromise:true (async returns null),
// so all network calls are kicked off in a sync evaluate that stashes
// the result on window, then we poll synchronously for it.
async function pageFetchJson(page, url) {
  const slot = '__r' + Math.random().toString(36).slice(2);
  await page.evaluate(
    ([slot, url]) => {
      window[slot] = { state: 'pending' };
      fetch(url, { credentials: 'include' })
        .then((r) => r.text().then((t) => ({ status: r.status, body: t })))
        .then((v) => { window[slot] = { state: 'done', value: v }; })
        .catch((e) => { window[slot] = { state: 'error', error: String(e) }; });
    },
    [slot, url],
  );
  for (let i = 0; i < 60; i++) {
    await new Promise((r) => setTimeout(r, 250));
    const got = await page.evaluate((s) => window[s], slot);
    if (got && got.state !== 'pending') {
      await page.evaluate((s) => { delete window[s]; }, slot);
      if (got.state === 'error') throw new Error('fetch error: ' + got.error);
      try { return JSON.parse(got.value.body); }
      catch { throw new Error('non-JSON body (status ' + got.value.status + '): ' + got.value.body.slice(0, 200)); }
    }
  }
  throw new Error('fetch timed out: ' + url);
}

async function main() {
  const browser = await chromium.connectOverCDP(CDP_WS);
  const context = browser.contexts()[0] || (await browser.newContext());
  const page = await context.newPage();
  try {
    await page.goto('https://passport.bilibili.com/login', {
      waitUntil: 'domcontentloaded',
      timeout: 60000,
    });

    const gen = await pageFetchJson(
      page,
      'https://passport.bilibili.com/x/passport-login/web/qrcode/generate',
    );
    if (gen.code !== 0 || !gen.data?.url) {
      throw new Error('qrcode/generate failed: ' + JSON.stringify(gen));
    }
    const { url, qrcode_key } = gen.data;

    console.log('\nScan this QR with the bilibili mobile app, then confirm:\n');
    qrcode.generate(url, { small: true });
    console.log(`\n(payload: ${url})\n`);

    const start = Date.now();
    let lastCode = null;
    let userInfo = null;
    while (Date.now() - start < MAX_WAIT_MS) {
      const poll = await pageFetchJson(
        page,
        'https://passport.bilibili.com/x/passport-login/web/qrcode/poll?qrcode_key=' +
          encodeURIComponent(qrcode_key),
      );
      const inner = poll?.data?.code;
      if (inner !== lastCode) {
        console.log(`[${new Date().toISOString()}] status ${inner}: ${STATUS[inner] || 'unknown'}`);
        lastCode = inner;
      }
      if (inner === 0) { userInfo = poll.data; break; }
      if (inner === 86038) throw new Error('QR code expired before confirmation');
      await new Promise((r) => setTimeout(r, POLL_INTERVAL_MS));
    }
    if (!userInfo) throw new Error('Timed out waiting for QR scan');

    const cookies = await context.cookies([
      'https://www.bilibili.com',
      'https://passport.bilibili.com',
      'https://api.bilibili.com',
    ]);
    fs.writeFileSync(COOKIE_FILE, JSON.stringify(cookies, null, 2));
    console.log(`\nSaved ${cookies.length} cookies to ${COOKIE_FILE}`);
    const have = (n) => cookies.some((c) => c.name === n);
    for (const n of ['SESSDATA', 'bili_jct', 'DedeUserID']) {
      console.log(`  ${have(n) ? 'ok' : 'MISSING'}  ${n}`);
    }

    // Verify with a server-side fetch using the saved cookies — calling
    // api.bilibili.com from inside the passport.bilibili.com page hits
    // SameSite/credentials quirks that say "not logged in" even when the
    // session is real.
    const cookieHeader = cookies.map((x) => `${x.name}=${x.value}`).join('; ');
    const nav = await fetch('https://api.bilibili.com/x/web-interface/nav', {
      headers: { cookie: cookieHeader, 'user-agent': 'Mozilla/5.0' },
    }).then((r) => r.json());
    if (nav?.data?.isLogin) {
      console.log(`\nLogged in as ${nav.data.uname} (mid=${nav.data.mid})`);
    } else {
      console.log('\nnav API says not logged in:', JSON.stringify(nav));
      process.exitCode = 1;
    }
  } finally {
    await page.close().catch(() => {});
    await browser.close().catch(() => {});
  }
}

main().catch((e) => {
  console.error('FAILED:', e.message);
  process.exit(1);
});
