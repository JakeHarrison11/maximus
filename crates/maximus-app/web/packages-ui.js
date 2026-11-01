import { listPackages, installPackage, removePackage } from "./packages.js";

const panel = document.createElement("details");
panel.id = "package-manager";
Object.assign(panel.style, {
    position: "fixed", right: "4px", bottom: "2px", zIndex: "30",
    background: "#252525", color: "#eee", padding: "8px", borderRadius: "5px",
    maxHeight: "75vh", maxWidth: "min(520px, 85vw)", overflow: "auto",
    fontSize: "13px", boxShadow: "0 2px 8px #0008",
});
const summary = document.createElement("summary");
summary.textContent = "Maxima packages";
summary.style.cursor = "pointer";
panel.appendChild(summary);
const explanation = document.createElement("p");
explanation.textContent = "Install optional groups before loading their entry files: install combinatorics, then load(\"combinatorics\"). Group and file names can differ (physics contains units.mac). Downloads are verified and kept offline. Files become available on the next evaluation. Removal may require Stop/restart to unload definitions. Native-tool packages may not work in the browser.";
panel.appendChild(explanation);
const status = document.createElement("p");
status.setAttribute("role", "status");
status.setAttribute("aria-live", "polite");
panel.appendChild(status);
const retry = document.createElement("button");
retry.textContent = "Refresh package list";
panel.appendChild(retry);
const rows = document.createElement("div");
panel.appendChild(rows);
document.body.appendChild(panel);
let busy = false;
let loaded = false;

const size = bytes => `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
const showError = error => {
    status.textContent = `Package error: ${error?.message ?? error}. Check your connection/storage permissions and retry.`;
    status.style.color = "#ffb4a9";
};

async function refresh() {
    try {
        const packages = await listPackages();
        rows.replaceChildren();
        for (const entry of packages) {
            const row = document.createElement("div");
            row.dataset.package = entry.name;
            Object.assign(row.style, { display: "flex", gap: "8px", alignItems: "center", marginTop: "6px" });
            const label = document.createElement("span");
            label.textContent = `${entry.name} — ${size(entry.size)}, ${entry.files} files${entry.installed ? " · installed/offline" : !entry.available ? " · no longer available; remove cached copy" : entry.outdated ? " · update required" : ""}`;
            label.style.flex = "1";
            row.appendChild(label);
            const button = document.createElement("button");
            button.textContent = entry.installed ? "Remove" : entry.outdated ? "Update" : "Install";
            button.disabled = busy || !entry.available;
            button.addEventListener("click", async () => {
                if (busy) return;
                busy = true;
                rows.querySelectorAll("button").forEach(control => { control.disabled = true; });
                status.style.color = "";
                status.textContent = `${entry.installed ? "Removing" : "Downloading"} ${entry.name}…`;
                try {
                    if (entry.installed) await removePackage(entry.name);
                    else await installPackage(entry.name, progress => {
                        status.textContent = `Downloading ${entry.name}: ${size(progress.bytes)} / ${size(progress.total)} (${progress.files}/${progress.totalFiles} files)`;
                    });
                    status.textContent = entry.installed
                        ? `${entry.name} removed from offline storage. Stop/restart to discard loaded definitions.`
                        : `${entry.name} installed and available offline. You can now evaluate load("…").`;
                } catch (error) {
                    showError(error);
                } finally {
                    busy = false;
                    await refresh();
                }
            });
            row.appendChild(button);
            if (entry.outdated) {
                const remove = document.createElement("button");
                remove.textContent = "Remove";
                remove.disabled = busy;
                remove.addEventListener("click", async () => {
                    if (busy) return;
                    busy = true;
                    try {
                        await removePackage(entry.name);
                        status.textContent = `${entry.name} removed.`;
                    } catch (error) {
                        showError(error);
                    } finally {
                        busy = false;
                        await refresh();
                    }
                });
                row.appendChild(remove);
            }
            rows.appendChild(row);
        }
        loaded = true;
    } catch (error) {
        showError(error);
    }
}

retry.addEventListener("click", () => { if (!busy) void refresh(); });
panel.addEventListener("toggle", () => {
    if (panel.open && !loaded) void refresh();
});
window.addEventListener("maximus-packages-error", event => showError(event.detail.message));
