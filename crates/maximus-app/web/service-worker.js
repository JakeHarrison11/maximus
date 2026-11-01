const CACHE_PREFIX = "maximus-shell-";
const MANIFEST_NAME = "asset-manifest.json";
const BUILD_ID = "__MAXIMUS_BUILD_ID__";

function inScope(relative) {
    const url = new URL(relative, self.registration.scope);
    if (url.origin !== self.location.origin || !url.href.startsWith(self.registration.scope)) {
        throw new Error(`Asset escapes service-worker scope: ${relative}`);
    }
    return url;
}

async function announce(type, details = {}) {
    const clients = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
    for (const client of clients) {
        client.postMessage({ type, ...details });
    }
}

async function verifyAsset(response, expected) {
    if (!response.ok || response.type === "opaque") {
        throw new Error(`Asset request failed (${response.status}): ${expected.url}`);
    }
    if (expected.sha256 && self.crypto?.subtle) {
        const digest = await self.crypto.subtle.digest("SHA-256", await response.clone().arrayBuffer());
        const actual = [...new Uint8Array(digest)]
            .map((value) => value.toString(16).padStart(2, "0"))
            .join("");
        if (actual !== expected.sha256.toLowerCase()) {
            throw new Error(`Asset checksum mismatch: ${expected.url}`);
        }
    }
    return response;
}

async function installShell() {
    const scope = self.registration.scope;
    const manifestResponse = await fetch(inScope(MANIFEST_NAME), { cache: "no-store" });
    if (!manifestResponse.ok) {
        throw new Error(`Could not load ${MANIFEST_NAME} (${manifestResponse.status})`);
    }
    const manifest = await manifestResponse.json();
    if (!manifest.build_id || manifest.build_id !== BUILD_ID
        || !Array.isArray(manifest.files) || manifest.files.length === 0) {
        throw new Error("Asset manifest is empty or has no build_id");
    }

    const files = new Set(manifest.files.map((asset) => new URL(asset.url, scope).pathname));
    for (const required of [
        "runtime/maxima.mjs",
        "runtime/maxima.data",
        "runtime/maxima.wasm",
        "runtime/help.info",
        "runtime/packages.json",
    ]) {
        if (![...files].some((path) => path.endsWith(`/${required}`))) {
            throw new Error(`Asset manifest is missing required runtime asset ${required}`);
        }
    }

    const cacheName = `${CACHE_PREFIX}${BUILD_ID}`;
    const cache = await caches.open(cacheName);
    try {
        for (const asset of manifest.files) {
            const url = inScope(asset.url);
            if (await cache.match(url)) {
                continue;
            }
            const response = await fetch(url, { cache: "no-store" });
            await verifyAsset(response, asset);
            await cache.put(url, response);
        }
        await cache.put(inScope(MANIFEST_NAME), new Response(JSON.stringify(manifest), {
            headers: { "content-type": "application/json" },
        }));
    } catch (error) {
        throw error;
    }
    await announce("offline-ready", {
        buildId: BUILD_ID,
        message: "Offline ready. Maximus is available without a network connection.",
    });
}

self.addEventListener("install", (event) => {
    event.waitUntil(installShell().catch(async (error) => {
        await announce("offline-error", { message: String(error?.message ?? error) });
        throw error;
    }));
});

self.addEventListener("activate", (event) => {
    event.waitUntil((async () => {
        const shellCaches = (await caches.keys()).filter((name) => name.startsWith(CACHE_PREFIX));
        const current = `${CACHE_PREFIX}${BUILD_ID}`;
        await Promise.all(shellCaches
            .filter((name) => name !== current)
            .map((name) => caches.delete(name)));
        await self.clients.claim();
        await announce("offline-ready", {
            buildId: BUILD_ID,
            message: "Offline ready. Maximus is available without a network connection.",
        });
    })());
});

self.addEventListener("fetch", (event) => {
    const request = event.request;
    if (request.method !== "GET" || request.headers.has("range")) {
        return;
    }
    const url = new URL(request.url);
    if (url.origin !== self.location.origin || !url.href.startsWith(self.registration.scope)) {
        return;
    }

    // Only cache immutable URLs beneath packages/<source-version>/.
    const optionalPackage = /\/runtime\/packages\/[^/]+\//.test(url.pathname);
    event.respondWith((async () => {
        const cacheName = `${CACHE_PREFIX}${BUILD_ID}`;
        const cache = (await caches.keys()).includes(cacheName)
            ? await caches.open(cacheName)
            : null;
        if (!optionalPackage && cache) {
            const cached = await cache.match(request);
            if (cached) {
                return cached;
            }
            if (request.mode === "navigate") {
                const index = await cache.match(inScope("index.html"));
                if (index) {
                    return index;
                }
            }
        }
        try {
            const response = await fetch(request);
            if (optionalPackage && response.ok && response.type !== "opaque" && cache) {
                await cache.put(request, response.clone());
            }
            return response;
        } catch (error) {
            const cached = cache ? await cache.match(request) : null;
            if (cached) {
                return cached;
            }
            await announce("offline-error", { message: String(error?.message ?? error) });
            throw error;
        }
    })());
});
