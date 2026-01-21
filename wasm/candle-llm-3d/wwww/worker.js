import init, { LLMEngine } from "../pkg/candle_llm_3d.js";

let engine = null;

self.onmessage = async (e) => {
    const { action, payload } = e.data;

    switch (action) {
        case "init":
            try {
                // Initialize WASM
                await init();

                // Initialize Engine with transferring buffers
                const { weights, tokenizer, config } = payload;
                engine = await LLMEngine.init(weights, tokenizer, config);

                self.postMessage({ action: "loaded" });
                console.log("[Worker] LLMEngine initialized.");
            } catch (err) {
                console.error("[Worker] Init failed:", err);
                self.postMessage({ action: "error", error: err.toString() });
            }
            break;

        case "generate":
            if (!engine) {
                self.postMessage({ action: "error", error: "Engine not initialized" });
                return;
            }

            try {
                const { prompt } = payload;
                engine.apply_prompt(prompt);

                while (!engine.is_finished()) {
                    const token = engine.step();
                    if (token) {
                        self.postMessage({ action: "token", token });
                    }
                    // No need for setTimeout here in a worker, but maybe good to yield occasionally if we want to process incoming messages?
                    // Actually, for a tightly coupled generation loop, blocking the worker thread is fine as long as we post messages.
                    // But if we want to support "stop" later, we might need to yield.
                    // For now, let's just run tight loop, or minimal yield.
                    // Using a small await allowing other events to process (like 'stop' if we implement it).
                    await new Promise(r => setTimeout(r, 0));
                }

                self.postMessage({ action: "done" });
            } catch (err) {
                console.error("[Worker] Generation failed:", err);
                self.postMessage({ action: "error", error: err.toString() });
            }
            break;

        default:
            console.warn("[Worker] Unknown action:", action);
    }
};
