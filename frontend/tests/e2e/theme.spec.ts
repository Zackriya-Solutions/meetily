import { expect, test } from '@playwright/test';
import { mockTauri } from './tauri-fixture';

const proofDir = process.env.THEME_PROOF_DIR;

test.afterEach(async ({ page, context }, testInfo) => {
  const video = page.video();
  await context.close();
  if (proofDir && video) await video.saveAs(`${proofDir}/${testInfo.title.replace(/[^a-z0-9]+/gi, '-').toLowerCase()}.webm`);
});

test('system appearance, manual override, persistence, and app surfaces', async ({ page }) => {
  const pageErrors: string[] = [];
  page.on('pageerror', error => pageErrors.push(error.message));
  await mockTauri(page);
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto('/settings');
  const appearance = page.getByRole('combobox', { name: 'Appearance', exact: true });
  await expect(appearance).toBeEnabled({ timeout: 15000 });
  await expect(appearance).toHaveValue('system');
  await expect(page.locator('html')).toHaveClass('dark');
  await expect(page.locator('body')).toHaveCSS('color-scheme', 'dark');
  await expect(page.getByRole('heading', { name: 'Notifications', exact: true }).locator('xpath=ancestor::div[contains(@class, "bg-white")][1]')).toHaveCSS('background-color', 'rgb(24, 24, 27)');
  if (proofDir) await page.screenshot({ path: `${proofDir}/01-dark-settings.png`, fullPage: true });

  await appearance.selectOption('light');
  await expect(page.locator('html')).toHaveClass('light');
  await expect(page.getByRole('heading', { name: 'Notifications', exact: true }).locator('xpath=ancestor::div[contains(@class, "bg-white")][1]')).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  if (proofDir) await page.screenshot({ path: `${proofDir}/02-light-settings.png`, fullPage: true });
  await page.reload();
  await expect(appearance).toHaveValue('light');
  await expect(page.locator('html')).toHaveClass('light');

  await appearance.selectOption('system');
  await expect(page.locator('html')).toHaveClass('dark');
  await page.emulateMedia({ colorScheme: 'light' });
  await expect(page.locator('html')).toHaveClass('light');
  await appearance.selectOption('dark');
  await page.emulateMedia({ colorScheme: 'light' });
  await expect(page.locator('html')).toHaveClass('dark');
  await page.reload();
  await expect(appearance).toHaveValue('dark');
  await expect(page.locator('html')).toHaveClass('dark');

  await page.getByRole('tab', { name: 'Recordings', exact: true }).click();
  await expect(page.getByText('Save Location', { exact: true })).toBeVisible();
  await expect(page.getByText('Save Location', { exact: true }).locator('..')).toHaveCSS('background-color', 'rgb(9, 9, 11)');
  if (proofDir) await page.screenshot({ path: `${proofDir}/03-dark-recordings.png`, fullPage: true });
  await page.getByRole('button', { name: 'About Meetily' }).first().click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await expect(page.getByRole('dialog')).toHaveCSS('opacity', '1');
  await expect(page.getByRole('dialog')).toHaveCSS('background-color', 'rgb(10, 10, 10)');
  if (proofDir) await page.screenshot({ path: `${proofDir}/04-dark-dialog.png`, fullPage: true });
  await page.keyboard.press('Escape');
  expect(pageErrors).toEqual([]);
});

test('first-run onboarding follows system dark mode', async ({ page }) => {
  const pageErrors: string[] = [];
  page.on('pageerror', error => pageErrors.push(error.message));
  await mockTauri(page, false);
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Welcome to Meetily' })).toBeVisible();
  await expect(page.locator('html')).toHaveClass('dark');
  await expect(page.getByText('Your data never leaves your device')).toHaveCSS('color', 'rgb(228, 228, 231)');
  if (proofDir) await page.screenshot({ path: `${proofDir}/05-dark-onboarding.png`, fullPage: true });
  expect(pageErrors).toEqual([]);
});

for (const format of ['markdown', 'blocknote']) {
  test(`${format} meeting editor follows the selected theme`, async ({ page }) => {
    const pageErrors: string[] = [];
    page.on('pageerror', error => pageErrors.push(error.message));
    await mockTauri(page);
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.goto(`/meeting-details?id=${format}`);
    await expect(page.getByText('Review the launch plan.', { exact: true })).toBeVisible();
    const editor = page.locator('.bn-container');
    await expect(editor).toHaveAttribute('data-color-scheme', 'dark');
    await expect(page.locator('.bn-editor')).not.toHaveCSS('background-color', 'rgb(255, 255, 255)');
    await expect(page.locator('main').locator('[style*="opacity"]').first()).toHaveCSS('opacity', '1');
    if (proofDir) await page.screenshot({ path: `${proofDir}/06-dark-${format}-editor.png`, fullPage: true });
    await page.goto('/settings');
    await page.getByRole('combobox', { name: 'Appearance', exact: true }).selectOption('light');
    await page.goto(`/meeting-details?id=${format}`);
    await expect(editor).toHaveAttribute('data-color-scheme', 'light');
    expect(pageErrors).toEqual([]);
  });
}

test('active model download remains visible in dark mode', async ({ page }) => {
  await mockTauri(page, false, 3);
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Transcription Engine' })).toBeVisible();
  await page.evaluate(() => (window as any).__themeTestEmit('parakeet-model-download-progress', {
    modelName: 'parakeet-tdt-0.6b-v3-int8', progress: 50, downloaded_mb: 335, total_mb: 670, speed_mbps: 10, status: 'downloading',
  }));
  const fill = page.locator('.bg-gradient-to-r').filter({ hasNot: page.locator('svg') }).first();
  await expect(fill).toHaveCSS('background-image', 'linear-gradient(to right, rgb(212, 212, 216), rgb(244, 244, 245))');
  await expect(fill).toHaveAttribute('style', 'width: 50%;');
  await expect(fill.locator('..')).toHaveCSS('background-color', 'rgb(63, 63, 70)');
  await expect.poll(async () => fill.evaluate(el => Math.round(el.getBoundingClientRect().width / el.parentElement!.getBoundingClientRect().width * 100))).toBe(50);
  await expect(page.getByRole('heading', { name: 'Getting things ready' })).toHaveCSS('opacity', '1');
  if (proofDir) await page.screenshot({ path: `${proofDir}/07-dark-active-download.png`, fullPage: true });
});
