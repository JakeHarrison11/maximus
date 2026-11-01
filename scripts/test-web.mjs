import assert from "node:assert/strict";
import { chromium } from "playwright";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const base = process.env.MAXIMUS_WEB_URL || "http://127.0.0.1:8765/";
const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({ acceptDownloads: true });
const page = await context.newPage();
page.on("filechooser", () => {});
const errors = [];
page.on("pageerror", error => errors.push(error.message));
page.on("console", message => {
    if (message.type() === "error") console.error(message.text());
});

async function stateValue(key, target = page) {
    return target.evaluate(async key => {
        const db = await new Promise((resolve, reject) => {
            const request = indexedDB.open("maximus-platform", 1);
            request.onsuccess = () => resolve(request.result);
            request.onerror = () => reject(request.error);
        });
        try {
            return await new Promise((resolve, reject) => {
                const request = db.transaction("state").objectStore("state").get(key);
                request.onsuccess = () => resolve(request.result ?? null);
                request.onerror = () => reject(request.error);
            });
        } finally { db.close(); }
    }, key);
}

async function recovery(target = page) {
    const contents = await stateValue("notebook", target);
    return contents ? JSON.parse(contents) : null;
}

async function waitForSaved(predicate, label, target = page) {
    const start = Date.now();
    while (Date.now() - start < 120000) {
        const notebook = await recovery(target);
        if (notebook && predicate(notebook)) return notebook;
        await target.waitForTimeout(500);
    }
    throw new Error(`Timed out: ${label}; recovery=${JSON.stringify(await recovery(target))}`);
}

async function openNotebook(notebook, name = "browser-test.maximus") {
    notebook.next_id = Math.max(0, ...notebook.cells.map(cell => cell.id)) + 1;
    const chooser = page.waitForEvent("filechooser");
    await page.locator("canvas").focus();
    await page.keyboard.press("Control+o");
    await (await chooser).setFiles({
        name, mimeType: "application/json", buffer: Buffer.from(JSON.stringify(notebook)),
    });
    await page.waitForTimeout(500);
}

function code(id, source, mode = "Maxima") {
    return { id, kind: "Code", mode, source, outputs: [], collapsed: false };
}

try {
    if (!process.argv.includes("--minimal-only")) {
        await page.goto(base);
        await page.waitForSelector("#startup", { state: "detached", timeout: 120000 });
        await page.locator("canvas").click({ position: { x: 300, y: 180 } });
        await page.waitForTimeout(1000);
        assert.equal(await page.evaluate(() => crossOriginIsolated), false);

        // Test the production transport/driver in an ordinary module worker.
        if (!process.argv.includes("--ui-only")) {
            const frames = await page.evaluate(async () => {
                const worker = new Worker(new URL("maxima-worker.js", document.baseURI), { type: "module" });
                try {
                    return await new Promise((resolve, reject) => {
                        const frames = [];
                        const timeout = setTimeout(() => reject(new Error("Worker timeout")), 120000);
                        worker.onerror = event => { clearTimeout(timeout); reject(new Error(event.message)); };
                        worker.onmessage = ({ data }) => {
                            if (data.type === "error") { clearTimeout(timeout); reject(new Error(data.message)); }
                            if (data.type === "ready") {
                                const requests = [
                                    { id: "1", kind: "eval", code: 'a:41$ a+1; integrate(x^2,x); print("α"); plot2d(sin(x),[x,-2,2]); plot3d(x*y,[x,-1,1],[y,-1,1]);' },
                                    { id: "2", kind: "eval", code: "determinant(matrix([1,2],[3,4])); 2^80; α:5$ α+1;" },
                                    { id: "3", kind: "eval", code: "1/0;" },
                                    { id: "4", kind: "eval", code: 'load("/plugins/probe/probe.mac")$ probe(6);' },
                                    { id: "5", kind: "vars" },
                                ];
                                for (const request of requests) worker.postMessage({ type: "request", ...request });
                            }
                            if (data.type === "frame") {
                                frames.push(data.line);
                                if (data.line.startsWith("@@MX\t5\tdone\t")) {
                                    clearTimeout(timeout);
                                    resolve(frames);
                                }
                            }
                        };
                        worker.postMessage({ type: "init", files: [{
                            path: "/plugins/probe/probe.mac",
                            data: Array.from(new TextEncoder().encode("probe(x):=x^2$")),
                        }] });
                    });
                } finally { worker.terminate(); }
            });
            const diagnostics = frames.filter(line => !line.includes("\tplot\t")).join("\n");
            assert.ok(frames.includes("@@MX\t1\tdone\tok"), diagnostics);
            assert.ok(frames.some(line => line.includes("\tresult\t") && line.endsWith("\t42")));
            assert.ok(frames.some(line => line.includes("α")), diagnostics);
            for (const result of ["-2", "1208925819614629174706176", "6", "36"]) {
                assert.ok(frames.some(line => line.includes("\tresult\t") && line.endsWith(`\t${result}`)), diagnostics);
            }
            assert.ok(frames.includes("@@MX\t3\tdone\terr"), diagnostics);
            assert.ok(frames.includes("@@MX\t4\tdone\tok"), diagnostics);
            assert.ok(frames.some(line => line.startsWith("@@MX\t5\tvars\t") && line.includes("\tv\ta\t41")), diagnostics);
            const plots = frames.filter(line => line.includes("\tplot\t")).map(line => {
                const escaped = line.split("\t")[3];
                const text = escaped.replace(/\\([nrt\\])/g, (_, c) => ({ n: "\n", r: "\r", t: "\t", "\\": "\\" })[c]);
                return JSON.parse(text);
            });
            assert.deepEqual(plots.map(plot => plot.kind), ["plot2d", "plot3d"]);
            assert.ok(plots[0].series[0].pts.length >= 400);
            console.log("PASS: real worker algebra, calculus, bignums, Unicode, variables, plugins, errors/recovery, 2D/3D plots");
        }

        await openNotebook({ version: 1, cells: [
            code(1, "a:41$ a+1;"),
            code(2, "\\frac{1}{3}+\\frac{1}{6}", "Math"),
            code(3, "\\int x^2\\,dx", "Math"),
        ] });
        await page.keyboard.press("Control+Shift+Enter");
        const evaluated = await waitForSaved(nb => nb.cells.length === 3 && nb.cells.every(cell => cell.outputs.length > 0), "evaluated notebook autosave");
        assert.ok(evaluated.cells[0].outputs.some(output => output.Result?.text === "42"));
        assert.ok(evaluated.cells[1].outputs.some(output => output.Result?.text === "1/2"));
        assert.ok(evaluated.cells[2].outputs.some(output => output.Result?.text === "x^3/3"));
        assert.ok(!evaluated.cells.some(cell => cell.outputs.some(output => output.Error)));
        const downloading = page.waitForEvent("download");
        await page.keyboard.press("Control+s");
        const download = await downloading;
        const downloaded = JSON.parse(await readFile(await download.path(), "utf8"));
        assert.deepEqual(downloaded.cells, evaluated.cells);
        console.log("PASS: actual app import, Maxima/textbook/LaTeX math, evaluation, autosave and download");

        for (const [menuY, extension, expected] of [[155, ".tex", "\\begin{document}"], [181, ".mac", "a:41"]]) {
            await page.mouse.click(18, 12);
            await page.waitForTimeout(200);
            await page.mouse.click(60, menuY);
            await page.waitForTimeout(200);
            const exporting = page.waitForEvent("download");
            await page.mouse.click(494, 385);
            const exported = await exporting;
            assert.ok(exported.suggestedFilename().endsWith(extension));
            assert.ok((await readFile(await exported.path(), "utf8")).includes(expected));
        }
        console.log("PASS: actual LaTeX and Maxima-script export dialogs and downloads");

        await page.mouse.click(269, 12);
        await page.waitForTimeout(200);
        await page.mouse.click(295, 39);
        await page.waitForTimeout(200);
        const pluginChooser = page.waitForEvent("filechooser");
        await page.mouse.click(95, 139);
        await (await pluginChooser).setFiles(fileURLToPath(new URL("../examples/plugins/physics", import.meta.url)));
        let importedFiles;
        for (let attempt = 0; attempt < 30; attempt++) {
            importedFiles = await stateValue("runtime_files");
            if (importedFiles?.some(file => file.path === "/plugins/physics/plugin.toml")) break;
            await page.waitForTimeout(200);
        }
        assert.ok(importedFiles?.some(file => file.path === "/plugins/physics/physics.mac"));
        await page.mouse.click(339, 34);
        await page.mouse.click(18, 12);
        await page.waitForTimeout(200);
        const workspaceChooser = page.waitForEvent("filechooser");
        await page.mouse.click(60, 245);
        await (await workspaceChooser).setFiles({
            name: "helper.mac", mimeType: "text/plain", buffer: Buffer.from("uploaded(x):=x+1$"),
        });
        for (let attempt = 0; attempt < 30; attempt++) {
            importedFiles = await stateValue("runtime_files");
            if (importedFiles?.some(file => file.path === "/workspace/helper.mac")) break;
            await page.waitForTimeout(200);
        }
        assert.ok(importedFiles?.some(file => file.path === "/workspace/helper.mac"));
        await openNotebook({ version: 1, cells: [code(1, 'kinetic_energy(2,3);load("/workspace/helper.mac")$ uploaded(4);')] }, "plugin.maximus");
        await page.keyboard.press("Control+Shift+Enter");
        await waitForSaved(nb => nb.cells[0]?.outputs.some(output => output.Result?.text === "5"), "plugin and workspace file initialization");
        assert.ok((await recovery()).cells[0].outputs.some(output => output.Result?.text === "9"));
        console.log("PASS: plugin/workspace file pickers, TOML validation, persistent files and startup code");

        const infinite = "a:99$ do x:x+1$";
        await openNotebook({ version: 1, cells: [code(1, infinite)] }, "stop.maximus");
        await page.keyboard.press("Control+Shift+Enter");
        await page.waitForTimeout(1000);
        await page.keyboard.press("Control+.");
        await waitForSaved(nb => nb.cells[0]?.source === infinite && nb.cells[0].outputs.length === 0, "Stop keeps inputs but clears outputs");
        await page.mouse.click(220, 100);
        await page.keyboard.press("Control+a");
        await page.keyboard.type("a;kinetic_energy(2,3);2+2;");
        await page.keyboard.press("Shift+Enter");
        const restarted = await waitForSaved(nb => nb.cells[0]?.outputs.some(output => output.Result?.text === "4"), "evaluation after Stop without reopening");
        assert.ok(restarted.cells[0].outputs.some(output => output.Result?.text === "a"), "Stop must discard CAS variables");
        assert.ok(restarted.cells[0].outputs.some(output => output.Result?.text === "9"), "Stop must reload persisted plugins");
        console.log("PASS: UI remains responsive during long calculation; Stop and next evaluation work");

        await page.waitForFunction(async () => {
            const registration = await navigator.serviceWorker.getRegistration();
            return Boolean(registration?.active);
        }, null, { timeout: 120000 });
        await context.setOffline(true);
        await page.reload();
        await page.waitForSelector("#startup", { state: "detached", timeout: 120000 });
        await page.keyboard.press("Control+Shift+Enter");
        await waitForSaved(nb => nb.cells[0]?.outputs.some(output => output.Result?.text === "4"), "offline reload and real evaluation");
        assert.equal(errors.length, 0, errors.join("\n"));
        console.log("PASS: installed app reloads and evaluates with networking disabled");
    }

    for (const touch of [true, false]) {
        const touchContext = await browser.newContext({ hasTouch: touch, viewport: { width: 460, height: 800 } });
        const touchPage = await touchContext.newPage();
        try {
            await touchPage.addInitScript(() => {
                if (!localStorage.getItem("maximus.settings")) {
                    localStorage.setItem("maximus.settings", JSON.stringify({ minimal: true }));
                }
                const NativeWorker = window.Worker;
                window.Worker = class extends NativeWorker {
                    constructor(...args) {
                        super(...args);
                        this.addEventListener("message", ({ data }) => {
                            if (data.type === "ready") window.testEngineReady = true;
                        });
                    }
                };
            });
            await touchPage.goto(base);
            await touchPage.waitForSelector("#startup", { state: "detached", timeout: 120000 });
            await touchPage.waitForFunction(() => window.testEngineReady, null, { timeout: 120000 });
            for (const [x, y] of [[228, 595], [314, 595], [55, 542], [400, 702]]) {
                if (touch) {
                    await touchPage.touchscreen.tap(x, y);
                } else {
                    // Keys must fire even when the mouse keeps moving during the press.
                    await touchPage.mouse.move(x - 12, y);
                    await touchPage.mouse.down();
                    await touchPage.mouse.move(x + 12, y, { steps: 4 });
                    await touchPage.mouse.up();
                }
                await touchPage.waitForTimeout(150);
            }
            await touchPage.screenshot({ path: "temp/web-minimal.png" });
            await waitForSaved(nb => nb.cells.some(cell => cell.outputs.some(output => output.Result?.text === "42")), "touch calculator evaluation", touchPage);
            await touchPage.screenshot({ path: "temp/web-minimal.png" });
            console.log(`PASS: minimal calculator evaluates 6 × 7 through real ${touch ? "touch" : "moving mouse"} keypad events`);
        } finally {
            await touchContext.close();
        }
    }
} catch (error) {
    await page.locator("#web-status").evaluate(element => { element.style.display = "none"; });
    await page.screenshot({ path: "temp/browser-failure.png" });
    throw error;
} finally {
    await browser.close();
}
