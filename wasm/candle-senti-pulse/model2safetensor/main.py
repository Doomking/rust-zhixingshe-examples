import torch
from transformers import AutoModelForSequenceClassification, AutoTokenizer
from safetensors.torch import save_file
import json
import os

# 这个模型专门针对中文“好/坏”二分类微调，非常准
# 虽然是二分类，但对于情绪检测来说比模糊的三分类更稳
MODEL_NAME = "uer/roberta-base-finetuned-jd-binary-chinese"
SAVE_DIR = "./converted_model"

# 强制使用镜像
os.environ["HF_ENDPOINT"] = "https://hf-mirror.com"

def convert():
    if not os.path.exists(SAVE_DIR):
        os.makedirs(SAVE_DIR)

    print(f"🚀 正在下载模型: {MODEL_NAME}...")

    try:
        # 加载
        model = AutoModelForSequenceClassification.from_pretrained(MODEL_NAME, trust_remote_code=True)
        tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME, trust_remote_code=True)

        # 1. 导出 config
        print("📦 正在生成 config.json...")
        config = model.config.to_dict()
        with open(os.path.join(SAVE_DIR, "config.json"), "w", encoding="utf-8") as f:
            json.dump(config, f, indent=2, ensure_ascii=False)

        # 2. 导出 tokenizer
        print("📝 正在生成 tokenizer.json...")
        tokenizer.save_pretrained(SAVE_DIR)

        # 3. 导出权重
        print("💾 正在生成 model.safetensors...")
        state_dict = model.state_dict()

        # 移除可能存在的 _orig_mod 等前缀（如果使用了 torch.compile）
        clean_state_dict = {k.replace("_orig_mod.", ""): v for k, v in state_dict.items()}

        save_file(clean_state_dict, os.path.join(SAVE_DIR, "model.safetensors"))

        print("\n" + "✨"*15)
        print("✅ 转换成功！")
        print(f"模型标签映射: {model.config.id2label}")
        print("✨"*15)
        print("\n注意：这个模型是二分类（0: 负面, 1: 正面）")
        print("如果你需要中性（Neutral），可以在 Rust 里判断：")
        print("当 scores[0] 和 scores[1] 差距很小时（比如都接近 0.5），设为中性。")

    except Exception as e:
        print(f"\n❌ 还是下载失败: {e}")
        print("\n💡 终极备选方案：")
        print("请直接在浏览器访问 https://hf-mirror.com/uer/roberta-base-finetuned-jd-binary-chinese")
        print("手动下载 pytorch_model.bin, config.json, vocab.txt 三个文件到本地文件夹")

if __name__ == "__main__":
    convert()
