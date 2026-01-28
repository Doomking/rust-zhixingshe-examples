import os

import requests
import torch
from safetensors.torch import save_file


def download_and_convert():
    model_url = (
        "https://github.com/ChaoningZhang/MobileSAM/raw/master/weights/mobile_sam.pt"
    )
    pt_path = "./model/mobile_sam.pt"
    output_path = "./model/mobile_sam.safetensors"

    if not os.path.exists("./model"):
        os.makedirs("./model")

    # 1. 下载官方权重
    if not os.path.exists(pt_path):
        print(f"📥 正在从官方 GitHub 下载 MobileSAM 权重 (约 40MB)...")
        response = requests.get(model_url, stream=True)
        with open(pt_path, "wb") as f:
            for chunk in response.iter_content(chunk_size=8192):
                f.write(chunk)
        print("✅ 下载完成")

    # 2. 转换为 Safetensors
    print(f"🔄 正在转换为 Safetensors 格式...")
    # map_location='cpu' 确保在没有 GPU 的机器上也能转换
    checkpoint = torch.load(pt_path, map_location="cpu")

    # 清理并保存
    # MobileSAM 的权重字典通常在 'state_dict' 键下，或者直接是字典
    state_dict = checkpoint.get("state_dict", checkpoint)

    save_file(state_dict, output_path)
    print(f"✨ 转换成功！文件已保存至: {output_path}")

    # 可选：删除原始 .pt 文件以节省空间
    # os.remove(pt_path)


if __name__ == "__main__":
    download_and_convert()
