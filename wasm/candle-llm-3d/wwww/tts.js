export class TextToSpeech {
    constructor(onStart, onEnd) {
        this.synth = window.speechSynthesis;
        this.queue = [];
        this.isPlaying = false;
        this.onStart = onStart;
        this.onEnd = onEnd;
        this.buffer = "";

        // 尝试获取中文语音
        this.voice = null;
        this.initVoice();

        // 某些浏览器(Chrome)需要用户交互才能播放，通常在点击发送按钮时已经满足
        // 监听 voicechanged 以便异步加载语音列表
        if (speechSynthesis.onvoiceschanged !== undefined) {
            speechSynthesis.onvoiceschanged = this.initVoice.bind(this);
        }
    }

    initVoice() {
        const voices = this.synth.getVoices();
        // 优先选择中文女声 (Google, Microsoft, or System default)
        this.voice = voices.find(v => v.lang.includes("zh") && (v.name.includes("Female") || v.name.includes("Google"))) ||
            voices.find(v => v.lang.includes("zh")) ||
            voices[0];
        console.log("[TTS] Selected voice:", this.voice ? this.voice.name : "Default");
    }

    // 流式输入：追加文本，按标点断句播放
    append(text) {
        this.buffer += text;
        this.processBuffer();
    }

    processBuffer() {
        // 极致连贯模式：只在整句结束时才朗读
        // 除非缓冲区堆积太满，否则绝不轻易切断
        const MAX_BUFFER_SIZE = 80; // 缓冲区警告阈值

        // 1. 优先检查强断句符号 (句号/感叹/问号) -> 这是最自然的停顿点
        const strongPunctuation = /[。！？.!?;]/;
        let match = strongPunctuation.exec(this.buffer);

        if (match) {
            const sentence = this.buffer.substring(0, match.index + 1).trim();
            this.buffer = this.buffer.substring(match.index + 1);
            if (sentence) this.enqueue(sentence);
            return;
        }

        // 2. 只有当积压太多文字时，才被迫在逗号处喘气 (防止延迟过高)
        if (this.buffer.length > MAX_BUFFER_SIZE) {
            const weakPunctuation = /[,，、]/;
            match = weakPunctuation.exec(this.buffer);
            if (match) {
                const sentence = this.buffer.substring(0, match.index + 1).trim();
                this.buffer = this.buffer.substring(match.index + 1);
                if (sentence) this.enqueue(sentence);
            }
        }
    } // 强制刷新剩余缓冲区（生成结束时调用）
    flush() {
        if (this.buffer.trim()) {
            this.enqueue(this.buffer);
            this.buffer = "";
        }
    }

    enqueue(text) {
        // 过滤掉所有可能被读出来的标点符号，替换为空格以保持停顿感
        const cleanText = text.replace(/[。！？；.!?,，、]/g, " ");
        this.queue.push(cleanText);
        this.playNext();
    }

    playNext() {
        if (this.isPlaying || this.queue.length === 0) return;

        this.isPlaying = true;
        const text = this.queue.shift();
        const utterance = new SpeechSynthesisUtterance(text);

        if (this.voice) utterance.voice = this.voice;
        utterance.rate = 1.0; // 恢复自然语速
        utterance.pitch = 1.1; // 稍微提高音调，更像少女

        utterance.onstart = () => {
            if (this.onStart) this.onStart();
        };

        utterance.onend = () => {
            this.isPlaying = false;
            if (this.queue.length === 0 && this.onEnd) {
                this.onEnd(); // 队列清空才停止口型
            }
            this.playNext();
        };

        utterance.onerror = (e) => {
            console.error("[TTS] Error:", e);
            this.isPlaying = false;
            this.playNext();
        };

        this.synth.speak(utterance);
    }

    stop() {
        this.synth.cancel();
        this.queue = [];
        this.buffer = "";
        this.isPlaying = false;
        if (this.onEnd) this.onEnd();
    }
}
