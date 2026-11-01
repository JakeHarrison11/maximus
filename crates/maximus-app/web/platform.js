// Browser-only services. No module-level DOM access: workers may import this module too.
// Storage is origin-scoped; importing a plugin never executes its code.
const DATABASE = "maximus-platform";
const SETTINGS_KEY = "maximus.settings";
const events = [];
let repaint = null;
let database;
let initialization;
let initialized = false;
let files = [];
let mutations = Promise.resolve();

function emit(event) {
    events.push(event);
    if (repaint) repaint();
}

export function reportError(operation, error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(`Maximus ${operation}: ${message}`);
    emit({ type: "error", operation, message });
}

function sync(operation, action) {
    try {
        return action();
    } catch (error) {
        reportError(operation, error);
        throw error;
    }
}

async function attempt(operation, action) {
    try {
        return await action();
    } catch (error) {
        reportError(operation, error);
        throw error;
    }
}

// Reject rather than normalize: the same path must mean the same thing in IndexedDB
// and the worker filesystem. Quotes/backslashes also cannot enter Maxima load strings.
export function validateRelativePath(path) {
    if (typeof path !== "string" || !path ||
        /[\\:"\x00-\x1f\x7f-\x9f]/u.test(path) ||
        path.split("/").some(part => !part || part === "." || part === "..")) {
        throw new Error(`Unsafe relative path: ${String(path)}`);
    }
    return path;
}

function validateFile(file) {
    if (!file || typeof file.path !== "string") throw new Error("Missing file path");
    const relative = validateRelativePath(file.path.slice(1));
    if (file.path[0] !== "/" ||
        !(relative.startsWith("plugins/") || relative.startsWith("workspace/"))) {
        throw new Error(`Unsupported mount path: ${file.path}`);
    }
    if (relative.startsWith("plugins/") && relative.split("/").length < 3) {
        throw new Error("Plugin files need a folder");
    }
    if (!Array.isArray(file.bytes) ||
        !file.bytes.every(byte => Number.isInteger(byte) && byte >= 0 && byte <= 255)) {
        throw new Error(`Invalid bytes: ${file.path}`);
    }
    return { path: file.path, bytes: file.bytes.slice() };
}

function validateFiles(input) {
    if (!Array.isArray(input)) throw new Error("Expected a file array");
    const checked = input.map(validateFile);
    if (new Set(checked.map(file => file.path)).size !== checked.length) {
        throw new Error("Duplicate file paths");
    }
    return checked.sort((a, b) => a.path.localeCompare(b.path));
}

function openDatabase() {
    return new Promise((resolve, reject) => {
        const request = indexedDB.open(DATABASE, 1);
        let blocked = false;
        request.onupgradeneeded = () => {
            request.result.createObjectStore("state");
        };
        request.onerror = () => reject(request.error);
        request.onblocked = () => {
            blocked = true;
            reject(new Error("Storage upgrade blocked; close other Maximus tabs and reload"));
        };
        request.onsuccess = () => {
            const db = request.result;
            if (blocked) {
                db.close();
                return;
            }
            db.onversionchange = () => {
                db.close();
                initialized = false;
                reportError("storage", "Storage changed in another tab; reload this page");
            };
            resolve(db);
        };
    });
}

function read(key) {
    return new Promise((resolve, reject) => {
        const transaction = database.transaction("state", "readonly");
        const request = transaction.objectStore("state").get(key);
        transaction.oncomplete = () => resolve(request.result);
        transaction.onabort = () => reject(transaction.error || new Error("Storage read aborted"));
        transaction.onerror = () => {}; // onabort reports the transaction failure.
    });
}

function write(key, value) {
    return new Promise((resolve, reject) => {
        const transaction = database.transaction("state", "readwrite");
        transaction.objectStore("state").put(value, key);
        transaction.oncomplete = () => resolve();
        transaction.onabort = () => reject(transaction.error || new Error("Storage write aborted"));
        transaction.onerror = () => {};
    });
}

export function setRepaintCallback(callback) {
    repaint = callback;
}

export function initPlatform() {
    if (!initialization) {
        initialization = attempt("initialize", async () => {
            database = await openDatabase();
            const stored = await read("runtime_files");
            files = validateFiles(stored === undefined ? [] : stored);
            initialized = true;
            emit({ type: "initialized" });
        });
    }
    return initialization;
}

function serialized(operation, action) {
    const pending = mutations.then(() => attempt(operation, async () => {
        await initPlatform();
        if (!initialized) throw new Error("Storage is unavailable; reload this page");
        return action();
    }));
    mutations = pending.catch(() => {});
    return pending;
}

export function takeEvents() {
    return JSON.stringify(events.splice(0));
}

export function loadSettings() {
    return sync("load settings", () => localStorage.getItem(SETTINGS_KEY));
}

export function saveSettings(json) {
    sync("save settings", () => {
        JSON.parse(json);
        localStorage.setItem(SETTINGS_KEY, json);
    });
}

async function readNotebookContents() {
    const contents = await read("notebook");
    if (contents !== undefined && typeof contents !== "string") {
        throw new Error("Stored notebook is not text");
    }
    return contents ?? null;
}

export function loadNotebook() {
    return serialized("load notebook", readNotebookContents);
}

export function restoreNotebook() {
    return serialized("restore notebook", async () => {
        emit({ type: "notebook_restored", contents: await readNotebookContents() });
    });
}

export function saveNotebook(contents) {
    return serialized("autosave notebook", async () => {
        JSON.parse(contents);
        await write("notebook", contents);
        emit({ type: "notebook_saved" });
    });
}

function selectFiles({ directory = false, multiple = false, accept = "" } = {}) {
    return new Promise((resolve, reject) => {
        const input = document.createElement("input");
        input.type = "file";
        input.multiple = multiple || directory;
        input.accept = accept;
        if (directory) input.setAttribute("webkitdirectory", "");
        input.style.display = "none";
        const finish = value => {
            input.remove();
            resolve(value);
        };
        input.onchange = () => finish(Array.from(input.files || []));
        input.oncancel = () => finish([]);
        document.body.appendChild(input);
        try {
            // Must run in the user's click handler, before any await.
            input.click();
        } catch (error) {
            input.remove();
            reject(error);
        }
    });
}

export function openNotebook() {
    return attempt("open notebook", async () => {
        const selected = await selectFiles({ accept: ".maximus,application/json" });
        if (!selected.length) return;
        const contents = await selected[0].text();
        JSON.parse(contents);
        emit({ type: "notebook_opened", name: selected[0].name, contents });
    });
}

export function exportNotebook(name, contents) {
    JSON.parse(contents);
    return exportText(name.endsWith(".maximus") ? name : `${name}.maximus`, contents);
}

export function exportText(name, contents) {
    return sync("export notebook", () => {
        validateRelativePath(name);
        if (name.includes("/")) throw new Error("Download name must not contain folders");
        const url = URL.createObjectURL(new Blob([contents], { type: "text/plain;charset=utf-8" }));
        const anchor = document.createElement("a");
        anchor.href = url;
        anchor.download = name;
        try {
            document.body.appendChild(anchor);
            anchor.click();
        } finally {
            anchor.remove();
            // Revoking immediately can cancel downloads in some browsers.
            setTimeout(() => URL.revokeObjectURL(url), 60_000);
        }
    });
}

async function uploadedFiles(selected, prefix) {
    return Promise.all(selected.map(async file => {
        const relative = validateRelativePath(file.webkitRelativePath || file.name);
        return { path: `${prefix}/${relative}`, bytes: Array.from(new Uint8Array(await file.arrayBuffer())) };
    }));
}

export function selectPluginDirectory() {
    return attempt("select plugin", async () => {
        const selected = await selectFiles({ directory: true });
        if (!selected.length) return;
        const imported = validateFiles(await uploadedFiles(selected, "/plugins"));
        const folder = imported[0].path.split("/")[2];
        if (imported.some(file => !file.path.startsWith(`/plugins/${folder}/`)) ||
            !imported.some(file => file.path === `/plugins/${folder}/plugin.toml`)) {
            throw new Error("Select one plugin folder containing plugin.toml");
        }
        // Rust parses TOML and checks referenced files before committing this candidate.
        emit({ type: "plugin_selected", folder, files: imported });
    });
}

export function commitPluginFiles(json) {
    return serialized("import plugin", async () => {
        const imported = validateFiles(JSON.parse(json));
        if (!imported.length) throw new Error("Plugin is empty");
        const folder = imported[0].path.split("/")[2];
        const prefix = `/plugins/${folder}/`;
        if (imported.some(file => !file.path.startsWith(prefix)) ||
            !imported.some(file => file.path === `${prefix}plugin.toml`)) {
            throw new Error("Expected exactly one plugin folder with plugin.toml");
        }
        await persistFiles([...files.filter(file => !file.path.startsWith(prefix)), ...imported]);
        emit({ type: "files_changed" });
    });
}

async function persistFiles(next) {
    const checked = validateFiles(next);
    await write("runtime_files", checked);
    files = checked; // Failed writes never alter the live snapshot.
}

export function uploadWorkspace(directory = false) {
    return attempt("upload workspace", async () => {
        const selected = await selectFiles({ directory, multiple: true });
        if (!selected.length) return;
        const imported = validateFiles(await uploadedFiles(selected, "/workspace"));
        await serialized("persist workspace", async () => {
            const paths = new Set(imported.map(file => file.path));
            await persistFiles([...files.filter(file => !paths.has(file.path)), ...imported]);
            emit({ type: "files_changed" });
        });
    });
}

export function removePlugin(folder) {
    return serialized("remove plugin", async () => {
        validateRelativePath(folder);
        if (folder.includes("/")) throw new Error("Expected a plugin folder name");
        await persistFiles(files.filter(file => !file.path.startsWith(`/plugins/${folder}/`)));
        emit({ type: "files_changed" });
    });
}

export function runtimeFilesSnapshot() {
    return sync("snapshot files", () => {
        if (!initialized) throw new Error("Await initPlatform before reading files");
        return JSON.stringify(files);
    });
}

// Worker/backend entry point: await this BEFORE initializing the CAS and mount every
// {path, bytes} in its filesystem. Stop/restart must not delete these persisted files.
// A worker import has its own module state; each call refreshes it from IndexedDB.
export function getRuntimeFiles() {
    return serialized("read runtime files", async () => {
        const stored = await read("runtime_files");
        files = validateFiles(stored === undefined ? [] : stored);
        return files.map(file => ({ path: file.path, bytes: file.bytes.slice() }));
    });
}

// The backend bridge need not resolve wasm-bindgen's generated snippet module URL.
// Register only on the main thread; worker module imports remain DOM-independent.
if (typeof window !== "undefined") {
    window.maximusRuntimeFiles = async () =>
        (await getRuntimeFiles()).map(file => ({ path: file.path, data: file.bytes }));
}
