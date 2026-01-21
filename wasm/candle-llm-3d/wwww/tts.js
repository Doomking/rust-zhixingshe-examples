export class TextToSpeech {
    constructor(onStart, onEnd) {
        this.synth = window.speechSynthesis;
        this.queue = [];
        this.isPlaying = false;
        this.onStart = onStart;
        this.onEnd = onEnd;
        this.buffer = "";

        // 获取中文语音
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
        console.log("[TTS] Available voices:", voices);
        this.voice = voices.find(v => v.lang.includes("zh") && (v.name.includes("美嘉") || v.name.includes("Google"))) ||
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
        // 极致响应模式：更积极地断句以降低延迟
        const MAX_BUFFER_SIZE = 10; // 缓冲区警告阈值：大幅降低

        // 包含逗号、顿号在内的所有停顿符号都直接触发播放
        const punctuation = /[。！？，；.!?,;]/;
        let match = punctuation.exec(this.buffer);

        if (match) {
            const sentence = this.buffer.substring(0, match.index + 1).trim();
            this.buffer = this.buffer.substring(match.index + 1);
            if (sentence) this.enqueue(sentence);
            return;
        }

        // 只有当无标点积压太多文字时，才强制切断
        if (this.buffer.length > MAX_BUFFER_SIZE) {
            const sentence = this.buffer.trim();
            this.enqueue(sentence);
            this.buffer = "";
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
