import os
import shutil
import json

def prepare_assets(src_dir, dest_dir, extension=".jpg"):
    # 1. 检查并创建目标目录
    if not os.path.exists(dest_dir):
        os.makedirs(dest_dir)
        print(f"已创建目标目录: {dest_dir}")

    # 2. 筛选有效的图片文件
    valid_extensions = ('.jpg', '.jpeg', '.png', '.webp')
    files = [f for f in os.listdir(src_dir) if f.lower().endswith(valid_extensions)]
    files.sort() # 排序确保编号稳定性

    # 3. 复制并重命名
    for i, filename in enumerate(files, start=1):
        src_path = os.path.join(src_dir, filename)
        new_name = f"img{i}{extension}"
        dest_path = os.path.join(dest_dir, new_name)
        
        # 使用 copy2 保留元数据，如果目标已存在会覆盖
        shutil.copy2(src_path, dest_path)
        print(f"已处理: {filename} -> {new_name}")

    # 4. 自动生成配置文件供前端读取
    config = {
        "count": len(files),
        "extension": extension,
        "prefix": "img"
    }
    
    config_path = os.path.join(os.path.dirname(dest_dir), "asset_config.json")
    with open(config_path, 'w') as f:
        json.dump(config, f, indent=4)

    print(f"\n✨ 处理完成！共计 {len(files)} 张图片。")
    print(f"配置文件已生成: {config_path}")

if __name__ == "__main__":
    # 配置你的路径
    # 源目录：你存放下载的原图的地方
    SOURCE = "./images" 
    # 目标目录：你的项目资产目录
    TARGET = "../www/assets/images"
    
    prepare_assets(SOURCE, TARGET)