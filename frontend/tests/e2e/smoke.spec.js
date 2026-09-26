import { test as base, expect } from '@playwright/test';

// Every test fails if the page throws, rejects a promise nobody handled,
// or the mock backend is asked for a command it doesn't know — the last
// one means api.js grew a command without a matching handler, which is
// exactly the kind of drift that only shows up in the real app.
const test = base.extend({
  page: async ({ page }, use) => {
    const problems = [];
    page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
    page.on('console', (m) => {
      if (m.type() === 'error' && /mock: no handler|Uncaught/.test(m.text())) problems.push(`console: ${m.text()}`);
    });
    await use(page);
    expect(problems, 'unexpected errors in the page').toEqual([]);
  }
});

async function boot(page, query = '') {
  await page.goto('/' + query);
  await expect(page.locator('.chrome')).toBeVisible();
  await expect(page.getByText('customer_revenue').first()).toBeVisible();
}

async function runTopCustomers(page) {
  await page.locator('.chrome').getByText('Top customers', { exact: true }).click();
  await page.locator('.view.show .toolbar .btn.primary').click();
  await expect(page.locator('.statusbar')).toContainText('15 rows');
}

async function openListPosts(page) {
  await page.locator('.chrome').getByText('List posts', { exact: true }).click();
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(page.getByText('200 OK').first()).toBeVisible();
}

test('boots with demo connections, tabs and schema tree', async ({ page }) => {
  await boot(page);
  const chrome = page.locator('.chrome');
  await expect(chrome.getByText('DEMO SHOP').first()).toBeVisible();
  await expect(chrome.getByText('List posts', { exact: true })).toBeVisible();
  for (const t of ['customers', 'order_items', 'orders', 'products']) {
    await expect(page.getByText(t, { exact: true }).first()).toBeVisible();
  }
});

test('SQL: running a query fills the result grid', async ({ page }) => {
  await boot(page);
  await runTopCustomers(page);
  await expect(page.locator('td', { hasText: 'Ava Ng' }).first()).toBeVisible();
  await expect(page.locator('th', { hasText: 'revenue' })).toBeVisible();
});

test('SQL: dragging across cells selects a rectangle, not text', async ({ page }) => {
  await boot(page);
  await runTopCustomers(page);
  const a = page.locator('td').filter({ hasText: 'Ava Ng' }).first();
  const b = page.locator('td').filter({ hasText: 'Austin' }).first();
  await a.hover();
  await page.mouse.down();
  await b.hover();
  await page.mouse.up();
  await expect(page.locator('td.sel')).toHaveCount(4);
  expect(await page.evaluate(() => String(window.getSelection()))).toBe('');
});

test('SQL: export saves a CSV with the result rows', async ({ page }) => {
  await boot(page);
  await runTopCustomers(page);
  await page.getByRole('button', { name: /^Export/ }).last().click();
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.getByRole('button', { name: 'Save as CSV…' }).click()
  ]);
  const text = await (await download.createReadStream()).toArray().then((c) => Buffer.concat(c).toString());
  expect(text.split('\n')[0]).toContain('name');
  expect(text).toContain('Ava Ng');
});

test('Requests: sending shows status and JSON body', async ({ page }) => {
  await boot(page);
  await openListPosts(page);
  await expect(page.getByText('"sunt aut facere repellat provident"').first()).toBeVisible();
});

test('Inspector: detail pane follows the selected node', async ({ page }) => {
  await boot(page);
  await openListPosts(page);
  await page.locator('.statusbar, .resp-bar, main').getByRole('button', { name: 'Inspector' }).first().click();
  const detail = page.locator('aside.detail');
  await page.locator('.row', { has: page.locator('.key', { hasText: /^title$/ }) }).click();
  await expect(detail.locator('.d-json')).toContainText('sunt aut facere');
  // Regression: the pane used to freeze on the first node clicked.
  await page.locator('.row', { has: page.locator('.key', { hasText: /^userId$/ }) }).click();
  await expect(detail.locator('.d-json')).not.toContainText('sunt aut facere');
  await expect(detail.locator('.d-json')).toHaveText('1');
});

test('Dialogs: typed text in a prompt is kept (new folder)', async ({ page }) => {
  await boot(page);
  await page.keyboard.press('Control+2');
  await page.locator('.view.show').getByTitle('New folder').click();
  const input = page.locator('.input').last();
  await input.fill('');
  // Type key by key — the old bug reset the value on every keystroke.
  await input.pressSequentially('E2E Folder', { delay: 20 });
  await expect(input).toHaveValue('E2E Folder');
  await input.press('Enter');
  await expect(page.getByText('E2E Folder', { exact: true })).toBeVisible();
});

test('Every tool panel and modal opens without backend errors', async ({ page }) => {
  await boot(page);
  await page.getByTitle('Settings').click();
  await expect(page.locator('.backdrop > .panel')).toBeVisible();
  await page.keyboard.press('Escape');
  await page.getByTitle('History & schedules').click();
  await expect(page.locator('.backdrop > .panel')).toBeVisible();
  await page.keyboard.press('Escape');

  await page.keyboard.press('Control+2');
  for (const name of ['Cookies', 'Network', 'Mock', 'WS', 'gRPC']) {
    await page.locator('.view.show').getByRole('button', { name, exact: true }).click();
    await expect(page.locator('.backdrop > .panel')).toBeVisible();
    await page.keyboard.press('Escape');
  }

  await page.keyboard.press('Control+3');
  await expect(page.getByRole('button', { name: 'Paste JSON' })).toBeVisible();
});

test('Appearance: ?theme=dark&colortheme=ocean applies the palette', async ({ page }) => {
  await boot(page, '?theme=dark&colortheme=ocean');
  const bg = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--surface-0').trim());
  expect(bg).toBe('#0d1b26');
});
