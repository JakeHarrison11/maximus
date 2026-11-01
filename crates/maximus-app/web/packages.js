// Explicit optional-package installation. Nothing in this module downloads a package
// until installPackage is called. IndexedDB, not the HTTP cache, owns installed bytes.
const CATALOG_URL = new URL("./runtime/packages.json", import.meta.url);
const DATABASE = "maximus-optional-packages";
let database;
let catalogPromise;
let snapshot;
let writes = Promise.resolve();

export function relativePath(path) {
    if (typeof path !== "string" || !path ||
        /[\\:"\x00-\x1f\x7f-\x9f]/u.test(path) ||
        path.split("/").some(part => !part || part === "." || part === "..")) {
        throw new Error(`Unsafe package path: ${String(path)}`);
    }
    return path;
}

export function validateCatalog(catalog, base = CATALOG_URL) {
    const root = new URL("./", base);
    if (!catalog || catalog.schema_version !== 1 ||
        typeof catalog.maxima_version !== "string" ||
        typeof catalog.packages !== "object" || !catalog.packages ||
        typeof catalog.manifest_url !== "string") {
        throw new Error("Invalid optional-package catalog");
    }
    const manifest = new URL(catalog.manifest_url, base);
    if (manifest.origin !== root.origin || !manifest.href.startsWith(new URL("./packages/", root).href)) {
        throw new Error("Package manifest must use the runtime packages directory");
    }
    const versionRoot = new URL("./", manifest);
    const paths = new Set();
    for (const [name, group] of Object.entries(catalog.packages)) {
        relativePath(name);
        if (name.includes("/") || group.virtual_root !== `/maxima/share/${name}` ||
            !Array.isArray(group.files) || !group.files.length) {
            throw new Error(`Invalid package group: ${name}`);
        }
        for (const file of group.files) {
            relativePath(file.path);
            const path = `${group.virtual_root}/${file.path}`;
            const url = new URL(file.url, base);
            if (paths.has(path) || typeof file.url !== "string" ||
                url.origin !== root.origin || !url.href.startsWith(versionRoot.href) ||
                url.search || url.hash || !/^[a-f0-9]{64}$/.test(file.sha256) ||
                !Number.isSafeInteger(file.size) || file.size < 0) {
                throw new Error(`Invalid package file: ${path}`);
            }
            paths.add(path);
        }
    }
    return catalog;
}

export async function verifyBytes(data, descriptor) {
    if (!(data instanceof Uint8Array) || data.length !== descriptor.size) {
        throw new Error(`Package size mismatch: ${descriptor.path}`);
    }
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", data));
    const hash = Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join("");
    if (hash !== descriptor.sha256) throw new Error(`Package integrity mismatch: ${descriptor.path}`);
}

function openDatabase() {
    if (!database) {
        database = new Promise((resolve, reject) => {
            const request = indexedDB.open(DATABASE, 1);
            let blocked = false;
            request.onupgradeneeded = () => {
                request.result.createObjectStore("groups", { keyPath: "name" });
                request.result.createObjectStore("meta");
            };
            request.onerror = () => reject(request.error);
            request.onblocked = () => {
                blocked = true;
                reject(new Error("Package storage blocked; close other Maximus tabs and reload"));
            };
            request.onsuccess = () => {
                const db = request.result;
                if (blocked) {
                    db.close();
                    return;
                }
                db.onversionchange = () => db.close();
                resolve(db);
            };
        });
    }
    return database;
}

async function readInstalled() {
    const db = await openDatabase();
    return new Promise((resolve, reject) => {
        const tx = db.transaction(["groups", "meta"], "readonly");
        const generation = tx.objectStore("meta").get("generation");
        const groups = tx.objectStore("groups").getAll();
        tx.oncomplete = () => resolve({ generation: generation.result ?? "", groups: groups.result });
        tx.onabort = () => reject(tx.error || new Error("Package storage read aborted"));
        tx.onerror = () => {};
    });
}

async function readGeneration() {
    const db = await openDatabase();
    return new Promise((resolve, reject) => {
        const tx = db.transaction("meta", "readonly");
        const request = tx.objectStore("meta").get("generation");
        tx.oncomplete = () => resolve(request.result ?? "");
        tx.onabort = () => reject(tx.error || new Error("Package storage read aborted"));
        tx.onerror = () => {};
    });
}

async function persist(name, group) {
    const db = await openDatabase();
    await new Promise((resolve, reject) => {
        const tx = db.transaction(["groups", "meta"], "readwrite");
        if (group) tx.objectStore("groups").put(group);
        else tx.objectStore("groups").delete(name);
        tx.objectStore("meta").put(crypto.randomUUID(), "generation");
        tx.oncomplete = resolve;
        tx.onabort = () => reject(tx.error || new Error("Package storage write aborted (possibly quota exceeded)"));
        tx.onerror = () => {};
    });
    snapshot = undefined;
}

function notify(type, detail) {
    if (typeof window !== "undefined") {
        window.dispatchEvent(new CustomEvent(`maximus-packages-${type}`, { detail }));
    }
}

async function operation(name, action) {
    try {
        return await action();
    } catch (error) {
        const message = `${name}: ${error?.message ?? error}`;
        console.error(`Maximus packages: ${message}`);
        notify("error", { message });
        throw error;
    }
}

export function getCatalog() {
    if (!catalogPromise) {
        catalogPromise = operation("read catalog", async () => {
            const response = await fetch(CATALOG_URL);
            if (!response.ok) throw new Error(`Catalog HTTP ${response.status}`);
            return validateCatalog(await response.json());
        }).catch(error => {
            catalogPromise = undefined;
            throw error;
        });
    }
    return catalogPromise;
}

export function listPackages() {
    return operation("list installed packages", async () => {
        const [catalog, stored] = await Promise.all([getCatalog(), readInstalled()]);
        const available = Object.entries(catalog.packages).map(([name, group]) => {
            const installed = stored.groups.find(record => record.name === name);
            return {
                name,
                files: group.files.length,
                size: group.files.reduce((sum, file) => sum + file.size, 0),
                installed: installed?.manifest_url === catalog.manifest_url,
                outdated: Boolean(installed && installed.manifest_url !== catalog.manifest_url),
                available: true,
            };
        });
        for (const installed of stored.groups) {
            if (!Object.hasOwn(catalog.packages, installed.name)) {
                available.push({
                    name: installed.name, files: installed.files.length,
                    size: installed.files.reduce((sum, file) => sum + file.data.length, 0),
                    installed: false, outdated: true, available: false,
                });
            }
        }
        return available;
    });
}

export function installPackage(name, onProgress = () => {}) {
    const pending = writes.then(() => operation(`install ${name}`, async () => {
        const catalog = await getCatalog();
        const group = Object.hasOwn(catalog.packages, name) ? catalog.packages[name] : null;
        if (!group) throw new Error(`Unknown package group: ${name}`);
        const files = [];
        let bytes = 0;
        const total = group.files.reduce((sum, file) => sum + file.size, 0);
        // Bound memory/network concurrency; never commit partially downloaded groups.
        for (const descriptor of group.files) {
            const response = await fetch(new URL(descriptor.url, CATALOG_URL), { cache: "no-cache" });
            if (!response.ok) throw new Error(`${descriptor.path}: HTTP ${response.status}`);
            const data = new Uint8Array(await response.arrayBuffer());
            await verifyBytes(data, descriptor);
            files.push({ path: `${group.virtual_root}/${descriptor.path}`, data });
            bytes += data.length;
            onProgress({ name, bytes, total, files: files.length, totalFiles: group.files.length });
        }
        await persist(name, { name, manifest_url: catalog.manifest_url, files });
        notify("changed", { name, installed: true });
    }));
    writes = pending.catch(() => {});
    return pending;
}

export function removePackage(name) {
    const pending = writes.then(() => operation(`remove ${name}`, async () => {
        relativePath(name);
        if (name.includes("/")) throw new Error("Expected one package group name");
        await persist(name, null);
        notify("changed", { name, installed: false });
    }));
    writes = pending.catch(() => {});
    return pending;
}

// Workers call this before init and each request. Catalog fetch is small and covered
// by the core offline asset cache; optional file bodies are NEVER fetched here.
export function getInstalledPackageFiles() {
    return operation("restore installed packages", async () => {
        if (snapshot && snapshot.generation === await readGeneration()) return snapshot.files;
        const stored = await readInstalled();
        // A fresh user has nothing to mount, so core startup needs no catalog fetch.
        if (!stored.groups.length) {
            snapshot = { generation: stored.generation, files: [] };
            return snapshot.files;
        }
        const catalog = await getCatalog();
        const files = [];
        for (const record of stored.groups) {
            if (record.manifest_url !== catalog.manifest_url) {
                throw new Error(`Installed ${record.name} belongs to another Maxima version; update or remove it in Packages`);
            }
            const group = Object.hasOwn(catalog.packages, record.name) ? catalog.packages[record.name] : null;
            if (!group || !Array.isArray(record.files) || record.files.length !== group.files.length) {
                throw new Error(`Invalid cached package: ${record.name}; remove and reinstall it`);
            }
            for (const descriptor of group.files) {
                const path = `${group.virtual_root}/${descriptor.path}`;
                const cached = record.files.find(file => file.path === path);
                if (!cached) throw new Error(`Cached package file missing: ${path}`);
                await verifyBytes(cached.data, descriptor);
                files.push({ path, data: cached.data });
            }
        }
        snapshot = { generation: stored.generation, files };
        return files;
    });
}
