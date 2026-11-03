export class MaximaEngine {
    constructor() {
        this.events = [];
        this.queue = [];
        this.ready = false;
        this.disposed = false;
        const status = message => {
            const element = document.getElementById("engine-status");
            if (element) element.textContent = message;
        };
        status("Loading Maxima worker…");
        this.worker = new Worker(new URL("maxima-worker.js", document.baseURI), { type: "module" });
        this.worker.onmessage = ({ data }) => {
            if (this.disposed) return;
            if (data.type === "progress") {
                status(data.message);
                return;
            }
            if (data.type === "ready") {
                this.ready = true;
                for (const request of this.queue.splice(0)) this.worker.postMessage(request);
            }
            if (["ready", "frame", "error"].includes(data.type)) this.events.push(data);
            else this.fail("Unknown Maxima worker event");
        };
        this.worker.onerror = (error) => this.fail(`Maxima worker failed: ${error.message}`);
        this.worker.onmessageerror = () => this.fail("Could not decode Maxima worker event");
        Promise.resolve().then(async () => {
            if (!window.maximusRuntimeFiles) throw new Error("Browser file storage was not initialized");
            const files = await window.maximusRuntimeFiles();
            if (!this.disposed) this.worker.postMessage({ type: "init", files });
        }).catch((error) => this.fail(`Could not restore Maxima files: ${error.message}`));
    }

    fail(message) {
        if (this.disposed) return;
        console.error(message);
        this.events.push({ type: "error", message });
        this.worker.terminate();
        this.ready = false;
    }

    submit(id, kind, code) {
        const request = { type: "request", id, kind, code };
        if (this.ready) this.worker.postMessage(request);
        else this.queue.push(request);
    }

    takeEvents() {
        return JSON.stringify(this.events.splice(0));
    }

    dispose() {
        this.disposed = true;
        this.queue = [];
        this.worker.terminate();
    }
}
