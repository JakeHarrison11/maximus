export class MaximaEngine {
    constructor() {
        this.events = [];
        this.queue = [];
        this.ready = false;
        this.disposed = false;
        this.requestChain = Promise.resolve();
        this.fileSnapshot = "";
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
        this.initialization = Promise.resolve().then(async () => {
            if (!window.maximusRuntimeFiles) throw new Error("Browser file storage was not initialized");
            const files = await window.maximusRuntimeFiles();
            this.fileSnapshot = JSON.stringify(files);
            if (!this.disposed) this.worker.postMessage({ type: "init", files });
        }).catch((error) => {
            this.fail(`Could not restore Maxima files: ${error.message}`);
        });
    }

    fail(message) {
        if (this.disposed || this.failed) return;
        this.failed = true;
        console.error(message);
        this.events.push({ type: "error", message });
        this.worker.terminate();
        this.ready = false;
    }

    submit(id, kind, code) {
        this.requestChain = this.requestChain.then(async () => {
            await this.initialization;
            if (this.failed || this.disposed) return;
            const files = await window.maximusRuntimeFiles();
            if (this.disposed) return;
            const snapshot = JSON.stringify(files);
            const request = { type: "request", id, kind, code };
            if (snapshot !== this.fileSnapshot) {
                request.files = files;
                this.fileSnapshot = snapshot;
            }
            if (this.ready) this.worker.postMessage(request);
            else this.queue.push(request);
        }).catch(error => this.fail(`Could not mount Maxima files: ${error.message}`));
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
