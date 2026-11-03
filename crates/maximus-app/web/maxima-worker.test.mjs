// node --test crates/maximus-app/web/maxima-worker.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const source = (await readFile(new URL("./maxima-worker.js", import.meta.url), "utf8"))
    .replace('import createMaxima from "./runtime/maxima.mjs";', "const createMaxima = globalThis.testMaxima;")
    .replace('import { getInstalledPackageFiles } from "./packages.js";',
        "const getInstalledPackageFiles = () => Promise.resolve(globalThis.testPackages);");
let instance = 0;

async function worker({ plan = "/maxima/src/init-cl.lisp\n", failAt } = {}) {
    const mounted = new Map();
    const messages = [];
    const calls = [];
    const progress = [];
    globalThis.testPackages = [];
    globalThis.testMaxima = async ({ print }) => ({
        FS: {
            readFile() { return plan; },
            mkdirTree() {},
            writeFile(path, bytes) { mounted.set(path, [...bytes]); },
            unlink(path) { mounted.delete(path); },
        },
        ccall(name, _result, _types, args) {
            calls.push([name, args]);
            if (name === failAt) throw new RangeError("Maximum call stack size exceeded");
            if (name === "evaluate_maxima" || name === "list_variables") {
                print(`@@MX\t${args[0]}\tdone\tok`);
            }
            return 1;
        },
    });
    const scope = { postMessage: message => message.type === "progress" ? progress.push(message.message) : messages.push(message) };
    globalThis.self = scope;
    await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}#${instance++}`);
    return {
        mounted,
        calls,
        progress,
        async send(data) {
            const start = messages.length;
            scope.onmessage({ data });
            for (let i = 0; i < 50 && messages.length === start; i++) {
                await new Promise(resolve => setImmediate(resolve));
            }
            assert.ok(messages.length > start, "worker must produce a response");
            return messages.slice(start);
        },
    };
}

test("mounts persisted packages at init and refreshes/removes files before evaluation", async () => {
    const instance = await worker();
    globalThis.testPackages = [{ path: "/maxima/share/demo/a.mac", data: new Uint8Array([0, 255]) }];
    assert.deepEqual(await instance.send({ type: "init", files: [
        { path: "/workspace/data.bin", data: [1, 128] },
    ] }), [{ type: "ready" }]);
    assert.deepEqual(instance.mounted.get("/maxima/share/demo/a.mac"), [0, 255]);
    assert.deepEqual(instance.mounted.get("/workspace/data.bin"), [1, 128]);
    globalThis.testPackages = [{ path: "/maxima/share/other/b.lisp", data: new Uint8Array([2]) }];
    const result = await instance.send({ type: "request", id: "4", kind: "eval", code: "1+1;" });
    assert.ok(result.some(message => message.line === "@@MX\t4\tdone\tok"));
    assert.equal(instance.mounted.has("/maxima/share/demo/a.mac"), false);
    assert.deepEqual(instance.mounted.get("/maxima/share/other/b.lisp"), [2]);
});

test("startup loads the build-time plan in order before reporting readiness", async () => {
    const instance = await worker({ plan: "/maxima/src/globals.fasc\n/maxima/src/numerical/slatec.lisp\n" });
    assert.deepEqual(await instance.send({ type: "init", files: [] }), [{ type: "ready" }]);
    assert.deepEqual(instance.calls, [
        ["initialize_maxima", []],
        ["load_maxima_file", ["/maxima/src/globals.fasc"]],
        ["load_maxima_file", ["/maxima/src/numerical/slatec.lisp"]],
        ["finish_maxima", []],
    ]);
    assert.ok(instance.progress.some(message => message.startsWith("Loading Maxima")));
    assert.match(instance.progress.at(-1), /Maxima ready in .*WASM .*ECL .*modules .*storage/);
});

test("startup stack errors identify the failing source and never report readiness", async t => {
    t.mock.method(console, "error", () => {});
    const instance = await worker({ failAt: "load_maxima_file" });
    const messages = await instance.send({ type: "init", files: [] });
    assert.equal(messages[0].type, "error");
    assert.match(messages[0].message, /loading \/maxima\/src\/init-cl.lisp: Maximum call stack size exceeded/);
    assert.ok(!messages.some(message => message.type === "ready"));
    assert.ok(!instance.calls.some(([name]) => name === "finish_maxima"));
});

test("invalid source-load plans fail explicitly before loading any source", async t => {
    t.mock.method(console, "error", () => {});
    for (const plan of ["", "/maxima/src/../escape.lisp", "/workspace/override.lisp"]) {
        const instance = await worker({ plan });
        const messages = await instance.send({ type: "init", files: [] });
        assert.equal(messages[0].type, "error");
        assert.match(messages[0].message, /Invalid Maxima source-load plan/);
        assert.deepEqual(instance.calls, [["initialize_maxima", []]]);
    }
});

test("user uploads cannot override optional package paths", async t => {
    t.mock.method(console, "error", () => {});
    const instance = await worker();
    const messages = await instance.send({ type: "init", files: [
        { path: "/maxima/share/evil/a.mac", data: [1] },
    ] });
    assert.equal(messages[0].type, "error");
    assert.match(messages[0].message, /Invalid virtual file path/);
    assert.equal(instance.mounted.size, 0);
});

test("worker also rejects unsafe package-provider mount paths", async t => {
    t.mock.method(console, "error", () => {});
    const instance = await worker();
    globalThis.testPackages = [{ path: "/maxima/share/demo/../escape", data: new Uint8Array() }];
    const messages = await instance.send({ type: "init", files: [] });
    assert.equal(messages[0].type, "error");
    assert.match(messages[0].message, /Invalid optional package path/);
    assert.equal(instance.mounted.size, 0);
});
