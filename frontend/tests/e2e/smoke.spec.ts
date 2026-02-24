import { expect, test } from "@playwright/test";

test("app root loads without server 500 page", async ({ page, baseURL }) => {
  const response = await page.goto(baseURL || "/", {
    waitUntil: "domcontentloaded",
  });

  expect(response).not.toBeNull();
  expect(response?.status()).toBeLessThan(500);

  const content = await page.content();
  expect(content).not.toContain("500");
});
