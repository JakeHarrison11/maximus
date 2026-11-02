import assert from "node:assert/strict";
import { chromium, webkit } from "playwright";

const base = process.env.MAXIMUS_WEB_URL || "http://127.0.0.1:8765/";
const engine = process.env.MAXIMUS_TEST_BROWSER || "chromium";
assert.ok(["chromium", "webkit"].includes(engine), `Unknown browser: ${engine}`);
const browser = await { chromium, webkit }[engine].launch({
    ...(engine === "webkit" ? { env: { ...process.env, JSC_maxPerThreadStackUsage: "262144" } } : {}),
    ...(process.env.MAXIMUS_TEST_BROWSER_EXECUTABLE ? { executablePath: process.env.MAXIMUS_TEST_BROWSER_EXECUTABLE } : {}),
});
try {
    const page = await browser.newPage();
    const diagnostics = [];
    page.on("console", message => {
        diagnostics.push(message.text());
    });
    // Keep the test independent of the Rust UI while retaining the worker's origin.
    const url = new URL("runtime-test.html", base).href;
    await page.route(url, route => route.fulfill({ contentType: "text/html", body: "<!doctype html><title>Runtime test</title>" }));
    await page.goto(url);
    const frames = await page.evaluate(async base => {
        const worker = new Worker(new URL("maxima-worker.js", base), { type: "module" });
        try {
            return await new Promise((resolve, reject) => {
                const frames = [];
                const timeout = setTimeout(() => reject(new Error("Maxima runtime timed out")), 180000);
                const fail = message => {
                    clearTimeout(timeout);
                    reject(new Error(message));
                };
                worker.onerror = event => fail(event.message);
                worker.onmessage = ({ data }) => {
                    if (data.type === "error") fail(data.message);
                    if (data.type === "ready") {
                        worker.postMessage({ type: "request", id: "1", kind: "eval", code: "integrate(x^2,x); 6*7;" });
                    }
                    if (data.type === "frame") {
                        frames.push(data.line);
                        if (data.line.startsWith("@@MX\t1\tdone\t")) {
                            clearTimeout(timeout);
                            resolve(frames);
                        }
                    }
                };
                worker.postMessage({ type: "init", files: [] });
            });
        } finally {
            worker.terminate();
        }
    }, base);
    assert.ok(frames.some(line => line.includes("\tresult\t") && line.includes("x^3/3")), JSON.stringify(frames));
    assert.ok(frames.some(line => line.includes("\tresult\t") && line.endsWith("\t42")), JSON.stringify(frames));
    assert.ok(frames.includes("@@MX\t1\tdone\tok"), JSON.stringify(frames));
    assert.ok(!diagnostics.some(line => /prlimit64|maximum.*stack|stack overflow/i.test(line)), diagnostics.join("\n"));
    console.log(`PASS: ${engine} boots Maxima and evaluates calculus and arithmetic without stack/syscall errors`);
} finally {
    await browser.close();
}
