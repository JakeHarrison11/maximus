import createMaxima from "./runtime/maxima.mjs";
import { getInstalledPackageFiles } from "./packages.js";

let runtime;
let initializing = false;
let currentId = "0";
let completed = false;
let tail = [];
let chain = Promise.resolve();
let packageSnapshot;
let packagePaths = new Set();

function output(line) {
    if (line.startsWith("@@MX\t")) {
        if (line.startsWith(`@@MX\t${currentId}\tdone\t`)) completed = true;
        self.postMessage({ type: "frame", line });
    } else {
        tail.push(line);
        if (tail.length > 10) tail.shift();
        console.log(line);
    }
}

function mount(files) {
    for (const { path, data } of files) {
        if (typeof path !== "string" || !/^\/(plugins|workspace)\//.test(path) ||
            /[\\:"\x00-\x1f\x7f-\x9f]/u.test(path) ||
            path.slice(1).split("/").some((part) => !part || part === ".." || part === ".")) {
            throw new Error(`Invalid virtual file path: ${path}`);
        }
        runtime.FS.mkdirTree(path.slice(0, path.lastIndexOf("/")));
        runtime.FS.writeFile(path, new Uint8Array(data));
    }
}

async function mountPackages() {
    // The provider validates every file against the current packaged catalog and
    // SHA-256 before returning it. User uploads cannot enter /maxima/share.
    const files = await getInstalledPackageFiles();
    if (files === packageSnapshot) return;
    const next = new Set(files.map(file => file.path));
    for (const path of packagePaths) {
        if (!next.has(path)) runtime.FS.unlink(path);
    }
    for (const { path, data } of files) {
        if (!path.startsWith("/maxima/share/") ||
            path.slice(1).split("/").some(part => !part || part === "." || part === "..") ||
            /[\\:"\x00-\x1f\x7f-\x9f]/u.test(path)) {
            throw new Error(`Invalid optional package path: ${path}`);
        }
        runtime.FS.mkdirTree(path.slice(0, path.lastIndexOf("/")));
        runtime.FS.writeFile(path, data);
    }
    packagePaths = next;
    packageSnapshot = files;
}

async function initialize(files) {
    if (runtime || initializing) throw new Error("Maxima was initialized twice");
    initializing = true;
    runtime = await createMaxima({
        locateFile: (name) => new URL(`runtime/${name}`, import.meta.url).href,
        print: output,
        printErr: (line) => { tail.push(line); console.error(line); },
    });
    if (runtime.ccall("initialize_maxima", "number", [], []) !== 1) {
        throw new Error(`Maxima initialization failed: ${tail.join("\n")}`);
    }
    mount(files);
    await mountPackages();
    self.postMessage({ type: "ready" });
}

async function request({ id, kind, code }) {
    if (!runtime || !/^\d+$/.test(id)) throw new Error("Invalid Maxima request");
    await mountPackages();
    currentId = id;
    completed = false;
    let ok;
    if (kind === "eval") {
        ok = runtime.ccall("evaluate_maxima", "number", ["string", "string"], [id, code]);
    } else if (kind === "vars") {
        ok = runtime.ccall("list_variables", "number", ["string"], [id]);
    } else {
        throw new Error(`Unsupported Maxima request: ${kind}`);
    }
    if (ok !== 1 || !completed) {
        self.postMessage({ type: "frame", line: `@@MX\t${id}\terror\tMaxima evaluation failed; interactive terminal input and native tools are not supported in the browser.` });
        self.postMessage({ type: "frame", line: `@@MX\t${id}\tdone\terr` });
    }
}

self.onmessage = ({ data }) => {
    chain = chain.then(async () => {
        if (data.type === "init") await initialize(data.files);
        else if (data.type === "request") await request(data);
        else throw new Error("Unknown Maxima worker request");
    }).catch((error) => {
        console.error(error);
        self.postMessage({ type: "error", message: `${error.message}\n${tail.join("\n")}` });
    });
};
