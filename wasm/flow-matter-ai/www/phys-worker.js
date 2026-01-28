import init, { PhysicsApp } from "../pkg/flow_matter_ai.js";

let app;

self.onmessage = async (e) => {
    const { type, data } = e.data;

    try {
        switch (type) {
            case "INIT":
                await init();
                const { width, height } = data;
                app = new PhysicsApp(width, height);
                self.postMessage({ type: "READY" });
                break;

            case "RESIZE":
                if (app) app.resize(data.width, data.height);
                break;

            case "UPDATE_PARAMS":
                if (app) app.update_physics_params(data.viscosity, data.density);
                break;

            case "INJECT":
                if (app) {
                    const { mask, imgW, imgH, offset_x, offset_y, display_w, display_h, scaled_w, scaled_h, material } = data;

                    // 极致精度：独立计算 X 和 Y 的缩放倍率
                    const unitScaleX = display_w / scaled_w;
                    const unitScaleY = display_h / scaled_h;

                    app.inject(
                        new Uint8Array(mask),
                        imgW,
                        imgH,
                        scaled_w,
                        scaled_h,
                        offset_x,
                        offset_y,
                        unitScaleX,
                        unitScaleY,
                        material
                    );
                }
                break;

            case "TRIGGER_COLLAPSE":
                if (app) app.trigger_collapse(data.avgAudio);
                break;

            case "RENDER":
                if (app) {
                    const { avgAudio, mouse_x, mouse_y } = data;
                    const particles = app.render_frame(avgAudio, mouse_x, mouse_y);
                    self.postMessage({ type: "TICK", particles }, [particles.buffer]);
                }
                break;
        }
    } catch (err) {
        console.error("PhysWorker Error:", err);
        self.postMessage({ type: "ERROR", error: err.message });
    }
};
