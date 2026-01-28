import init, { InferenceApp } from "../pkg/flow_matter_ai.js";

let app;

self.onmessage = async (e) => {
    const { type, data } = e.data;

    try {
        switch (type) {
            case "INIT":
                await init();
                app = new InferenceApp(data.modelData);
                self.postMessage({ type: "READY" });
                break;

            case "SET_IMAGE":
                if (app) {
                    await app.set_image(new Uint8Array(data.imageBytes));
                    self.postMessage({ type: "IMAGE_READY" });
                }
                break;

            case "INTERACT":
                if (app) {
                    const { x, y, bounds } = data;
                    const result = await app.get_mask_at(x, y);
                    // 确保是 Uint8Array 以便进行 Transferable 传输
                    const mask = new Uint8Array(result.mask);
                    self.postMessage({
                        type: "MASK_READY",
                        mask,
                        material: result.material,
                        scaled_w: result.scaled_w,
                        scaled_h: result.scaled_h,
                        bounds
                    }, [mask.buffer]);
                }
                break;
        }
    } catch (err) {
        console.error("AIWorker Error:", err);
        self.postMessage({ type: "ERROR", error: err.message });
    }
};
