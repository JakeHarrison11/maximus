// Requires scripts/package.json dependencies and a served built web app.
// MAXIMUS_WEB_URL=http://127.0.0.1:8765/ node scripts/test-web-packages.mjs
import { chromium } from "playwright";
import assert from "node:assert/strict";

const browser = await chromium.launch({ headless: true });
try {
    const context = await browser.newContext();
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto(process.env.MAXIMUS_WEB_URL || "http://127.0.0.1:8765/");
    await page.locator("#package-manager summary").click();
    await page.waitForFunction(() => document.querySelectorAll("#package-manager [data-package]").length > 0);
    // Keep an engine alive while installing to verify before-request synchronization.
    await page.evaluate(async () => {
        window.packageTestWorker = new Worker("./maxima-worker.js", { type: "module" });
        await new Promise((resolve, reject) => {
            const timeout = setTimeout(() => reject(new Error("Worker initialization timeout")), 120_000);
            packageTestWorker.onerror = event => { clearTimeout(timeout); reject(new Error(event.message)); };
            packageTestWorker.onmessage = ({ data }) => {
                if (data.type === "ready") {
                    // Populate Maxima's search machinery before installing. Newly mounted
                    // directories must remain discoverable after a failed earlier load.
                    packageTestWorker.postMessage({ type: "request", id: "90", kind: "eval",
                        code: 'load("combinatorics");' });
                }
                if (data.type === "frame" && data.line.startsWith("@@MX\t90\tdone\t")) {
                    clearTimeout(timeout);
                    if (data.line.endsWith("\terr")) resolve();
                    else reject(new Error("Optional package unexpectedly present before installation"));
                }
                if (data.type === "error") { clearTimeout(timeout); reject(new Error(data.message)); }
            };
            packageTestWorker.postMessage({ type: "init", files: [] });
        });
    });
    const group = page.locator('#package-manager [data-package="combinatorics"]');
    await group.getByRole("button", { name: "Install", exact: true }).click();
    await group.getByRole("button", { name: "Remove", exact: true }).waitFor({ timeout: 120_000 });

    async function evaluatePackage(createWorker) {
        return page.evaluate(async create => {
            const worker = create ? new Worker("./maxima-worker.js", { type: "module" }) : window.packageTestWorker;
            const frames = [];
            return await new Promise((resolve, reject) => {
                const timeout = setTimeout(() => { worker.terminate(); reject(new Error("Package evaluation timeout")); }, 120_000);
                const send = () => worker.postMessage({ type: "request", id: "91", kind: "eval",
                    code: 'load("combinatorics");stirling2(5,2);' });
                worker.onerror = event => { clearTimeout(timeout); worker.terminate(); reject(new Error(event.message)); };
                worker.onmessage = ({ data }) => {
                    if (data.type === "ready") send();
                    if (data.type === "error") { clearTimeout(timeout); worker.terminate(); reject(new Error(data.message)); }
                    if (data.type === "frame") {
                        frames.push(data.line);
                        if (data.line.startsWith("@@MX\t91\tdone\t")) {
                            clearTimeout(timeout);
                            worker.terminate();
                            resolve(frames);
                        }
                    }
                };
                if (create) worker.postMessage({ type: "init", files: [] });
                else send();
            });
        }, createWorker);
    }

    const live = await evaluatePackage(false);
    assert.ok(live.includes("@@MX\t91\tdone\tok"), live.join("\n"));
    assert.ok(live.some(line => line.endsWith("\t15")), live.join("\n"));
    await page.waitForFunction(async () => {
        const registration = await navigator.serviceWorker.getRegistration();
        return Boolean(registration?.active && navigator.serviceWorker.controller);
    }, undefined, { timeout: 120_000 });
    await context.setOffline(true);
    await page.reload();
    await page.locator("#package-manager summary").click();
    await page.locator('#package-manager [data-package="combinatorics"]')
        .getByRole("button", { name: "Remove", exact: true }).waitFor({ timeout: 120_000 });
    const offline = await evaluatePackage(true);
    assert.ok(offline.includes("@@MX\t91\tdone\tok"), offline.join("\n"));
    assert.ok(offline.some(line => line.endsWith("\t15")), offline.join("\n"));
    assert.deepEqual(errors, []);
    console.log("PASS package UI install, live worker refresh, offline reload and worker restart");
} finally {
    await browser.close();
}
