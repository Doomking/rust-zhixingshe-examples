"""
TFLite to SafeTensors Converter (Direct Extraction)
Optimized for BlazeHand model structure.
"""
import re
import numpy as np
import tensorflow as tf
import torch
from safetensors.torch import save_file

def convert():
    tflite_path = "./model/hand_landmarks_detector.tflite"
    output_safetensors = "./model/hand_landmarks_detector.safetensors"

    print(f"🚀 Loading TFLite model from: {tflite_path}")
    interpreter = tf.lite.Interpreter(model_path=tflite_path, experimental_preserve_all_tensors=True)
    interpreter.allocate_tensors()

    # CRITICAL FIX: Run dummy inference to populate Dequantize tensors
    # Many weights are outputs of Dequantize ops and are null until evaluated.
    print("⚡ Running dummy inference to compute dequantized weights...")
    input_details = interpreter.get_input_details()
    for input_detail in input_details:
        input_shape = input_detail['shape']
        input_dtype = input_detail['dtype']
        # Create dummy input
        dummy = np.zeros(input_shape, dtype=input_dtype)
        interpreter.set_tensor(input_detail['index'], dummy)
    
    interpreter.invoke()
    print("✅ Inference complete. Weights should be ready.")

    details = interpreter.get_tensor_details()
    weights_dict = {}
    bn_to_conv_map = {} # Map BN index (str) to Target Layer Name (e.g. "depthwise_conv2d_9")

    # 1. 提取所有权重 (Extract all weights)
    # TFLite 权重通常带有 "dequantize" 后缀或者是只读资源
    for detail in details:
        name = detail['name']
        index = detail['index']
        
        # 过滤感兴趣的 tensor：卷积层、BatchNorm 层、全连接层
        # 我们主要关注包含 "model_1/model/" 的层
        if "model_1/model/" not in name:
            continue

        # 排除中间激活值
        is_param = False
        if "Conv2D" in name: is_param = True
        if "depthwise" in name: is_param = True
        if "FusedBatchNormV3" in name: is_param = True
        if "MatMul" in name: is_param = True
        if "BiasAdd/ReadVariableOp" in name: is_param = True
        
        # 如果是 dequantize，肯定是权重或参数
        if "_dequantize" in name:
            is_param = True

        if not is_param:
            continue

        try:
            data = interpreter.get_tensor(index)
            # 过滤掉空的或者显然不是权重的 (比如 shape 为空)
            if data.size == 0: continue
            
            # 清理名称
            clean_name = name
            clean_name = clean_name.replace('_dequantize', '')
            
            # 移除复杂的后缀并构建 BN Map
            if ';' in clean_name:
                parts = clean_name.split(';')
                
                # Try to extract BN index and Conv index from the fused name
                bn_part = next((p for p in parts if "batch_normalization" in p), None)
                conv_part = next((p for p in parts if "Conv2D" in p or "depthwise" in p), None)
                
                if bn_part and conv_part:
                    bn_match = re.search(r'batch_normalization_(\d+)', bn_part)
                    
                    # Determine target conv name
                    target_layer = None
                    dw_match = re.search(r'depthwise_conv2d_(\d+)', conv_part)
                    cv_match = re.search(r'conv2d_(\d+)', conv_part)
                    
                    if dw_match:
                        target_layer = f"depthwise_conv2d_{dw_match.group(1)}"
                    elif cv_match:
                        target_layer = f"conv2d_{cv_match.group(1)}"
                        
                    if bn_match and target_layer:
                        bn_idx = bn_match.group(1)
                        bn_to_conv_map[bn_idx] = target_layer
                        # print(f"  🔗 Found Fused Link: BN_{bn_idx} -> {target_layer}")

                # 优先级策略: Depthwise > Conv2D > model_1/model/
                dw_part = next((p for p in parts if "depthwise" in p), None)
                conv_part = next((p for p in parts if "Conv2D" in p), None)
                generic_part = next((p for p in parts if "model_1/model/" in p), None)
                
                if dw_part:
                    clean_name = dw_part
                elif conv_part:
                    clean_name = conv_part
                elif generic_part:
                    clean_name = generic_part
            
            # 进一步标准清理
            # 注意：不要移除 /depthwise 或 /Conv2D，因为 regex 依赖它们来区分类型
            clean_name = clean_name.replace('/BiasAdd/ReadVariableOp/resource', '_bias')
            clean_name = clean_name.replace('/MatMul', '')
            clean_name = clean_name.replace('/FusedBatchNormV3', '')
            
            weights_dict[clean_name] = data
        except Exception:
            pass

    print(f"✅ Extracted {len(weights_dict)} potential weight tensors")

    # 2. 转换为 Rust 格式 (Map to Rust names)
    rust_weights = {}

    # Helper function to add tensor
    def add_tensor(rust_name, data):
        if "weight" in rust_name and data.ndim == 4:
            # Candle layout: [Out, In, H, W] for Conv or [C, 1, H, W] for PW
            # Check last two dims (H, W)
            h, w = data.shape[2], data.shape[3]
            if h > 10 or w > 10: # Kernel size usually < 10. 
                # print(f"  ⚠️ Skipping likely activation: {rust_name} {data.shape}")
                return

        # Bias 必须是 1D
        if "bias" in rust_name and data.ndim != 1:
            # print(f"  ⚠️ Skipping likely weight assigned as bias: {rust_name} {data.shape}")
            return
            
        # 确保数据是连续的 float32
        tensor = torch.from_numpy(data.copy()).float().contiguous()
        
        # 防止覆盖
        if rust_name in rust_weights:
            return
            
        rust_weights[rust_name] = tensor
        print(f"  Mapped: {rust_name} <- {data.shape}")

    # ... (Step A and B and C are same) ...
    # Skip Step A/B/C modification here as we only changed add_tensor logic above.
    
    # D. Heads
    if "model_1/model/conv_landmarks" in weights_dict:
        # ...
        pass
    
    # E. Debug/Fallback for Biases: Scan ALL 1D tensors
    print("🔍 Scanning for ALL 1D tensors (Potential Biases)...")
    bias_candidates = []
    for detail in details:
         try:
             d = interpreter.get_tensor(detail['index'])
             if d.ndim == 1 and d.shape[0] > 1: # Ignore scalar or empty
                 bias_candidates.append((detail['name'], d))
         except: pass
         
    # Try to match unmatched biases by shape?
    # For now just print them to see what's available
    for name, data in bias_candidates:
        if "batch_normalization" in name or "bias" in name.lower():
             print(f"  Found 1D tensor: {name} ({data.shape})")

    # A. STEM Layer
    # model_1/model/conv2d -> model_1.model.conv2d
    # Note: Name might be "model_1/model/conv2d" or "model_1/model/conv2d/Conv2D"
    stem_weight = None
    if "model_1/model/conv2d" in weights_dict:
        stem_weight = weights_dict["model_1/model/conv2d"]
    elif "model_1/model/conv2d/Conv2D" in weights_dict:
        stem_weight = weights_dict["model_1/model/conv2d/Conv2D"]
        
    if stem_weight is not None:
        w = stem_weight
        if w.ndim == 4:
            # TFLite Conv2D: [Out, H, W, In] -> [Out, In, H, W]
            w = w.transpose(0, 3, 1, 2)
        add_tensor("model_1.model.conv2d.weight", w)

    if "model_1/model/batch_normalization" in weights_dict:
        b = weights_dict["model_1/model/batch_normalization"]
        add_tensor("model_1.model.conv2d.bias", b)

    # B. Regular Conv2D Layers (conv2d_1 to conv2d_X)
    # Search for any tensor containing model_1/model/conv2d_(\d+)
    # Note: iterating all items to find partial matches
    for name, data in weights_dict.items():
        # Check for Conv2D
        match = re.search(r'model_1/model/conv2d_(\d+)(/|$|;)', name)
        if match and "depthwise" not in name: # Ensure it's not depthwise if naming is ambiguous
            idx = match.group(1)
            # Ensure we haven't already processed this as a bias
            # TFLite Conv2D weights usually don't have "BiasAdd" in name, but raw check is safer
            
            # Check shape to confirm it's weight [Out, H, W, In] (4D) or [Out, In] (2D)
            # If 1D, it might be bias, skip here
            if data.ndim < 2: continue
            
            w = data
            if w.ndim == 4:
                # Transpose [Out, H, W, In] -> [Out, In, H, W]
                w = w.transpose(0, 3, 1, 2)
            
            key = f"model_1.model.conv2d_{idx}.weight"
            add_tensor(key, w)
            
            # Try to find bias by looking for a fused BN/Bias tensor in weights_dict
            # that contains the same conv2d index
            # Strategy: scan weights_dict for 1D tensor with matching name pattern
            for b_name, b_data in weights_dict.items():
                if b_data.ndim == 1 and f"conv2d_{idx}" in b_name:
                     # Check if it's really the bias for this layer (size match)
                     if b_data.shape[0] == w.shape[0]:
                         add_tensor(f"model_1.model.conv2d_{idx}.bias", b_data)
                         break

    # C. Depthwise Conv2D Layers
    for name, data in weights_dict.items():
        match = re.search(r'model_1/model/depthwise_conv2d_(\d+)(/|$|;)', name)
        if match:
            idx = match.group(1)
            
            if data.ndim < 2: continue
            
            w = data
            # Depthwise TFLite: [1, H, W, C]
            # Candle Depthwise: [C, 1, H, W]
            # Transpose: [1, H, W, C] -> [C, 1, H, W]
            # Permute: (3, 0, 1, 2)
            if w.ndim == 4:
                w = w.transpose(3, 0, 1, 2)
            
            add_tensor(f"model_1.model.depthwise_conv2d_{idx}.weight", w)
            
            # Look for bias
            for b_name, b_data in weights_dict.items():
                if b_data.ndim == 1 and f"depthwise_conv2d_{idx}" in b_name:
                     if b_data.shape[0] == w.shape[0]:
                         add_tensor(f"model_1.model.depthwise_conv2d_{idx}.bias", b_data)
                         break

    # Re-scan for fused names to link Bias/BN to Conv
    print(f"🔍 Linking Biases (Map size: {len(bn_to_conv_map)})...")
    for detail in details:
        name = detail['name']
        if not name: continue
        
        # Check if it's a BN/Bias tensor
        if ("FusedBatchNormV3" in name or "BiasAdd" in name) and "_dequantize" in name:
            try:
                data = interpreter.get_tensor(detail['index'])
            except: continue
            
            # Skip weights (check dim) (Must be 1D)
            if data.ndim != 1: continue

            target_key = None
            
            # Strategy 1: Direct Regex on Name (if it contains conv/depthwise)
            conv_match = re.search(r'conv2d_(\d+)', name)
            dw_match = re.search(r'depthwise_conv2d_(\d+)', name)
            stem_match = re.search(r'model_1/model/conv2d/', name) or re.search(r'model_1/model/conv2d;', name)
            
            if dw_match:
                 idx = dw_match.group(1)
                 target_key = f"model_1.model.depthwise_conv2d_{idx}.bias"
            elif conv_match:
                 idx = conv_match.group(1)
                 target_key = f"model_1.model.conv2d_{idx}.bias"
            elif stem_match:
                 target_key = "model_1.model.conv2d.bias"
            
            # Strategy 2: Use BN Map
            if not target_key:
                bn_match = re.search(r'batch_normalization_(\d+)', name)
                if bn_match:
                    bn_idx = bn_match.group(1)
                    if bn_idx in bn_to_conv_map:
                        layer_name = bn_to_conv_map[bn_idx]
                        target_key = f"model_1.model.{layer_name}.bias"
                        # print(f"  Matched via BN Map: BN_{bn_idx} -> {target_key}")

            if target_key:
                if target_key not in rust_weights:
                     add_tensor(target_key, data)

    # D. Heads
    if "model_1/model/conv_landmarks" in weights_dict:
        w = weights_dict["model_1/model/conv_landmarks"]
        if w.ndim == 4:
            w = w.squeeze() 
        add_tensor("model_1.model.conv_landmarks.weight", w)

    # E. Fallback: Shape & Proximity Matching for Missing Biases
    print("🔍 Running Final Fallback for Biases (Shape + Proximity)...")
    
    # 1. 找出所有已提取权重但缺少 Bias 的 Conv 层
    missing_bias_layers = [] 
    for key in list(rust_weights.keys()):
        if "weight" in key and ("conv2d" in key or "depthwise" in key):
            bias_key = key.replace("weight", "bias")
            if bias_key not in rust_weights:
                w_tensor = rust_weights[key]
                out_channels = w_tensor.shape[0] # [Out, In, H, W] or [Out=C, 1, H, W]
                missing_bias_layers.append((bias_key, out_channels, key))

    if missing_bias_layers:
        print(f"  👉 Found {len(missing_bias_layers)} layers missing bias: {[m[0] for m in missing_bias_layers]}")
        
        # 2. 收集所有未被使用的 1D Tensor (Potential Biases)
        candidate_biases = []
        for detail in details:
             name = detail['name']
             # 只看 BN/Bias 相关的
             if not (("batch_normalization" in name or "bias" in name.lower()) and "_dequantize" in name): continue
             
             try:
                 d = interpreter.get_tensor(detail['index'])
             except: continue
             
             if d.ndim == 1:
                 candidate_biases.append((detail, d))
        
        # 3. 尝试匹配
        for bias_key, channels, weight_key in missing_bias_layers:
             # 提取 weight index (从 key 中提取数字)以用于 proximity
             conv_idx = 0
             idx_match = re.search(r'_(\d+)\.', weight_key)
             if idx_match: conv_idx = int(idx_match.group(1))
             
             # 筛选出 Shape 匹配的 candidates
             matches = []
             for detail, d in candidate_biases:
                 if d.shape[0] == channels:
                     matches.append((detail, d))
             
             if len(matches) == 1:
                 # 唯一匹配
                 best_name = matches[0][0]['name']
                 print(f"  🎯 Fallback Matched (Unique Shape): {bias_key} <- {best_name}")
                 add_tensor(bias_key, matches[0][1])
             elif len(matches) > 1:
                 # 多个匹配，使用 Proximity
                 best_candidate = None
                 min_dist = 9999
                 best_name = ""
                 
                 for detail, d in matches:
                     # 尝试从 candidate name 提取数字
                     bn_match = re.search(r'batch_normalization_(\d+)', detail['name'])
                     if bn_match:
                         bn_idx = int(bn_match.group(1))
                         # Heuristic: BN index usually close to Conv Index
                         dist = abs(bn_idx - conv_idx)
                         # Priority to BN index > Conv Index (usually sequential)
                         if dist < min_dist:
                             min_dist = dist
                             best_candidate = d
                             best_name = detail['name']
                 
                 if best_candidate is not None:
                      print(f"  🎯 Fallback Matched (Index Proximity {min_dist}): {bias_key} <- {best_name}")
                      add_tensor(bias_key, best_candidate)

    # Bias via resource lookup or name
    # "model_1/model/conv_landmarks/BiasAdd/ReadVariableOp/resource"
    # We already cleaned names, so search keys
    for key, data in weights_dict.items():
        if "conv_landmarks" in key and data.shape == (63,):
            add_tensor("model_1.model.conv_landmarks.bias", data)
            break

    save_file(rust_weights, output_safetensors)
    print(f"✨ Successfully saved {len(rust_weights)} tensors to {output_safetensors}")

if __name__ == "__main__":
    convert()
