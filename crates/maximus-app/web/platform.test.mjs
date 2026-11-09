// Run with: node --test crates/maximus-app/web/platform.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const source = await readFile(new URL("./platform.js", import.meta.url), "utf8");
let instance = 0;
const freshPlatform = () => import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}#${instance++}`);

// Minimal transaction mock: completion, not request success, confirms persistence.
function storage() {
    const records = new Map();
    const state = { failWrites: false };
    const database = {
        createObjectStore() {},
        close() {},
        transaction() {
            const transaction = {
                objectStore: () => ({
                    get(key) {
                        const request = {};
                        queueMicrotask(() => {
                            request.result = structuredClone(records.get(key));
                            transaction.oncomplete();
                        });
                        return request;
                    },
                    put(value, key) {
                        queueMicrotask(() => {
                            if (state.failWrites) {
                                transaction.error = new Error("Quota exceeded");
                                transaction.onabort();
                            } else {
                                records.set(key, structuredClone(value));
                                transaction.oncomplete();
                            }
                        });
                    },
                }),
            };
            return transaction;
        },
    };
    globalThis.indexedDB = {
        open() {
            const request = { result: database };
            queueMicrotask(() => request.onsuccess());
            return request;
        },
    };
    return state;
}

test("path validation rejects traversal and Maxima string injection", async () => {
    const platform = await freshPlatform();
    for (const path of ["", "/a", "../a", "a/../b", "a//b", "a\\b", "a/", "a:\"b", "a\nb"]) {
        assert.throws(() => platform.validateRelativePath(path), /Unsafe relative path/);
    }
    assert.equal(platform.validateRelativePath("Physics/sub dir/α.mac"), "Physics/sub dir/α.mac");
});

test("initialization gates restore/save and files persist across module restart", async t => {
    storage();
    t.mock.method(console, "error", () => {});
    const platform = await freshPlatform();
    await assert.rejects(async () => platform.runtimeFilesSnapshot(), /Await initPlatform/);
    await platform.saveNotebook('{"cells":[]}');
    await platform.restoreNotebook();
    const events = JSON.parse(platform.takeEvents());
    assert.ok(events.findIndex(event => event.type === "initialized") <
        events.findIndex(event => event.type === "notebook_saved"));
    assert.deepEqual(events.at(-1), { type: "notebook_restored", contents: '{"cells":[]}' });
    const files = [
        { path: "/plugins/physics/plugin.toml", bytes: [110, 97, 109, 101] },
        { path: "/plugins/physics/sub/a.mac", bytes: [0, 127, 255] },
    ];
    await platform.commitPluginFiles(JSON.stringify(files));
    const restarted = await freshPlatform();
    assert.deepEqual(await restarted.getRuntimeFiles(), files);
    const snapshot = await restarted.getRuntimeFiles();
    snapshot[0].bytes[0] = 0;
    assert.deepEqual(JSON.parse(restarted.runtimeFilesSnapshot()), files);
    await restarted.removePlugin("physics");
    assert.deepEqual(await restarted.getRuntimeFiles(), []);
});

test("failed writes keep live files intact and report to the event queue and console", async t => {
    const state = storage();
    const consoleErrors = t.mock.method(console, "error", () => {});
    const platform = await freshPlatform();
    await platform.initPlatform();
    state.failWrites = true;
    await assert.rejects(platform.commitPluginFiles(JSON.stringify([
        { path: "/plugins/demo/plugin.toml", bytes: [] },
    ])), /Quota exceeded/);
    assert.deepEqual(JSON.parse(platform.runtimeFilesSnapshot()), []);
    assert.ok(JSON.parse(platform.takeEvents()).some(event =>
        event.type === "error" && event.operation === "import plugin" && /Quota/.test(event.message)));
    assert.equal(consoleErrors.mock.callCount(), 1);
    // A rejection must not poison the serial queue.
    state.failWrites = false;
    await platform.saveNotebook("{}");
});

test("settings failures and initialization failures are visible", async t => {
    t.mock.method(console, "error", () => {});
    globalThis.localStorage = { getItem() { throw new Error("Storage denied"); } };
    const platform = await freshPlatform();
    assert.throws(() => platform.loadSettings(), /Storage denied/);
    assert.equal(JSON.parse(platform.takeEvents())[0].operation, "load settings");
    globalThis.indexedDB = { open() { throw new Error("IndexedDB denied"); } };
    await assert.rejects(platform.getRuntimeFiles(), /IndexedDB denied/);
    assert.ok(JSON.parse(platform.takeEvents()).some(event => event.operation === "initialize"));
});

test("invalid mounts and duplicate files never reach persistence", async t => {
    storage();
    t.mock.method(console, "error", () => {});
    const platform = await freshPlatform();
    for (const path of ["/workspace/../a", "/plugins/demo/../../x", "/other/a", "/plugins/../plugin.toml"]) {
        await assert.rejects(platform.commitPluginFiles(JSON.stringify([{ path, bytes: [] }])));
    }
    const file = { path: "/plugins/demo/plugin.toml", bytes: [] };
    await assert.rejects(platform.commitPluginFiles(JSON.stringify([file, file])), /Duplicate/);
    assert.deepEqual(await platform.getRuntimeFiles(), []);
});

test("directory pickers preserve relative paths and workspace binary bytes", async () => {
    storage();
    globalThis.window = {};
    let selected = [];
    globalThis.document = {
        body: { appendChild() {} },
        createElement() {
            return {
                style: {},
                files: selected,
                setAttribute() {},
                remove() {},
                click() { queueMicrotask(() => this.onchange()); },
            };
        },
    };
    const upload = (path, bytes) => ({
        name: path.split("/").at(-1),
        webkitRelativePath: path,
        arrayBuffer: async () => Uint8Array.from(bytes).buffer,
    });
    const platform = await freshPlatform();
    selected = [upload("physics/plugin.toml", [110]), upload("physics/sub/a.mac", [255])];
    await platform.selectPluginDirectory();
    const candidate = JSON.parse(platform.takeEvents())[0];
    assert.equal(candidate.type, "plugin_selected");
    assert.equal(candidate.folder, "physics");
    assert.deepEqual(candidate.files.map(file => file.path),
        ["/plugins/physics/plugin.toml", "/plugins/physics/sub/a.mac"]);
    assert.deepEqual(await platform.getRuntimeFiles(), []); // Rust must validate first.
    selected = [upload("experiment/sub/data.bin", [0, 128, 255])];
    await platform.uploadWorkspace(true);
    const restarted = await freshPlatform();
    assert.deepEqual(await restarted.getRuntimeFiles(),
        [{ path: "/workspace/experiment/sub/data.bin", bytes: [0, 128, 255] }]);
    assert.deepEqual(await window.maximusRuntimeFiles(),
        [{ path: "/workspace/experiment/sub/data.bin", data: [0, 128, 255] }]);
});

test("PDF download preserves binary bytes, MIME type and filename", async t => {
    let downloaded;
    const anchor = { click: t.mock.fn(), remove: t.mock.fn() };
    const document = {
        body: { appendChild: t.mock.fn() },
        createElement(tag) {
            assert.equal(tag, "a");
            return anchor;
        },
    };
    const previousDocument = globalThis.document;
    globalThis.document = document;
    t.after(() => { globalThis.document = previousDocument; });
    t.mock.method(URL, "createObjectURL", blob => {
        downloaded = blob;
        return "blob:pdf-download";
    });
    t.mock.method(globalThis, "setTimeout", () => {});

    const platform = await freshPlatform();
    const bytes = Uint8Array.from([37, 80, 68, 70, 45, 0, 127, 128, 226, 255]);
    platform.exportBytes("notebook.pdf", bytes);

    assert.equal(downloaded.type, "application/pdf");
    assert.deepEqual(new Uint8Array(await downloaded.arrayBuffer()), bytes);
    assert.equal(anchor.download, "notebook.pdf");
    assert.equal(anchor.href, "blob:pdf-download");
    assert.equal(anchor.click.mock.callCount(), 1);
    assert.equal(anchor.remove.mock.callCount(), 1);
});
