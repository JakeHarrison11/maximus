// node --test crates/maximus-app/web/packages.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createHash, webcrypto } from "node:crypto";

if (!globalThis.crypto) globalThis.crypto = webcrypto;
const base = "https://maximus.test/runtime/packages.json";
const source = (await readFile(new URL("./packages.js", import.meta.url), "utf8"))
    .replace('new URL("./runtime/packages.json", import.meta.url)', `new URL("${base}")`);
let instance = 0;
const fresh = () => import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}#${instance++}`);
const bytes = new Uint8Array([0, 128, 255]);
const descriptor = {
    path: "sub/demo.mac", size: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
    url: "./packages/v1/demo/sub/demo.mac",
};
const catalog = () => ({
    schema_version: 1, maxima_version: "v1", manifest_url: "./packages/v1/manifest.json",
    packages: { demo: { virtual_root: "/maxima/share/demo", files: [structuredClone(descriptor)] } },
});

function setup() {
    const groups = new Map();
    const meta = new Map();
    const state = { failWrites: false, offline: false, corruptDownload: false, requests: [], groups };
    const db = {
        close() {},
        transaction(_names, mode) {
            let scheduled = false;
            let commit = () => {};
            const tx = {
                objectStore(name) {
                    const records = name === "groups" ? groups : meta;
                    const schedule = () => {
                        if (scheduled) return;
                        scheduled = true;
                        setImmediate(() => {
                            if (mode === "readwrite" && state.failWrites) {
                                tx.error = new Error("Quota exceeded");
                                tx.onabort();
                            } else {
                                commit();
                                tx.oncomplete();
                            }
                        });
                    };
                    return {
                        get(key) {
                            schedule();
                            return { result: structuredClone(records.get(key)) };
                        },
                        getAll() {
                            schedule();
                            return { result: structuredClone([...records.values()]) };
                        },
                        put(value, key) {
                            const prior = commit;
                            commit = () => { prior(); records.set(key ?? value.name, structuredClone(value)); };
                            schedule();
                        },
                        delete(key) {
                            const prior = commit;
                            commit = () => { prior(); records.delete(key); };
                            schedule();
                        },
                    };
                },
            };
            return tx;
        },
    };
    globalThis.indexedDB = {
        open() {
            const request = { result: db };
            queueMicrotask(() => request.onsuccess());
            return request;
        },
    };
    globalThis.fetch = async url => {
        state.requests.push(String(url));
        if (String(url) === base) return Response.json(catalog());
        if (state.offline) throw new Error("Network offline");
        return new Response(state.corruptDownload ? new Uint8Array([1, 2, 3]) : bytes);
    };
    return state;
}

test("catalog validates mount roots, traversal, origin and integrity descriptors", async () => {
    const module = await fresh();
    assert.equal(module.validateCatalog(catalog(), base).packages.demo.virtual_root, "/maxima/share/demo");
    for (const change of [
        c => { c.packages.demo.virtual_root = "/workspace/demo"; },
        c => { c.packages.demo.files[0].path = "../escape.mac"; },
        c => { c.packages.demo.files[0].url = "https://other.test/x"; },
        c => { c.packages.demo.files[0].sha256 = "bad"; },
        c => { c.packages.demo.files[0].size = -1; },
        c => { c.packages.demo.files.push(c.packages.demo.files[0]); },
        c => { c.manifest_url = "../wrong.json"; },
    ]) {
        const invalid = catalog();
        change(invalid);
        assert.throws(() => module.validateCatalog(invalid, base));
    }
});

test("install verifies binary files, reports progress, persists and restores offline", async () => {
    const state = setup();
    const module = await fresh();
    assert.deepEqual(await module.getInstalledPackageFiles(), []);
    assert.equal(state.requests.length, 0, "fresh startup must not fetch optional assets or catalog");
    assert.equal((await module.listPackages())[0].installed, false);
    const progress = [];
    await module.installPackage("demo", event => progress.push(event));
    assert.equal(progress[0].bytes, 3);
    assert.equal((await module.listPackages())[0].installed, true);
    state.offline = true;
    const restarted = await fresh();
    const files = await restarted.getInstalledPackageFiles();
    assert.deepEqual(files, [{ path: "/maxima/share/demo/sub/demo.mac", data: bytes }]);
    assert.equal(state.requests.filter(url => url.includes("/sub/demo.mac")).length, 1,
        "restoration must never redownload installed package bodies");
    assert.strictEqual(await restarted.getInstalledPackageFiles(), files, "unchanged snapshots reuse verified bytes");
    await restarted.removePackage("demo");
    assert.deepEqual(await restarted.getInstalledPackageFiles(), []);
});

test("corrupt downloads and storage failures do not install partial groups", async t => {
    t.mock.method(console, "error", () => {});
    const state = setup();
    const module = await fresh();
    state.corruptDownload = true;
    await assert.rejects(module.installPackage("demo"), /integrity mismatch/);
    assert.deepEqual(await module.getInstalledPackageFiles(), []);
    state.corruptDownload = false;
    state.failWrites = true;
    await assert.rejects(module.installPackage("demo"), /Quota exceeded/);
    assert.deepEqual(await module.getInstalledPackageFiles(), []);
    state.failWrites = false;
    await module.installPackage("demo");
    assert.equal((await module.listPackages())[0].installed, true, "failed operations must not poison the queue");
});

test("worker restoration rejects tampered and version-mismatched offline records", async t => {
    t.mock.method(console, "error", () => {});
    const state = setup();
    const module = await fresh();
    await module.installPackage("demo");
    state.groups.get("demo").files[0].data[0] = 9;
    await assert.rejects((await fresh()).getInstalledPackageFiles(), /integrity mismatch/);
    state.groups.get("demo").manifest_url = "./packages/old/manifest.json";
    await assert.rejects((await fresh()).getInstalledPackageFiles(), /another Maxima version/);
});

test("failed network downloads are explicit and unknown groups cannot fetch", async t => {
    t.mock.method(console, "error", () => {});
    const state = setup();
    const module = await fresh();
    state.offline = true;
    await assert.rejects(module.installPackage("demo"), /Network offline/);
    await assert.rejects(module.installPackage("__proto__"), /Unknown package/);
    assert.deepEqual(await module.getInstalledPackageFiles(), []);
});
