with open('src/render.rs', 'r') as f:
    content = f.read()

import re

# 1. Update the obstacle loop to skip text area
old_obstacle = """        for gy in 0..ny {
            for gx in 0..nx {
                if !fluid.is_solid(gx, gy) {
                    continue;
                }
                let x0 = (gx as i32 * cell).max(0);"""
new_obstacle = """        for gy in 0..ny {
            for gx in 0..nx {
                if !fluid.is_solid(gx, gy) {
                    continue;
                }
                // 不绘制隐藏用于文字碰撞的内部宏观网格
                if gy >= 10 && gy <= 13 && gx >= 8 && gx <= 23 {
                    continue;
                }
                let x0 = (gx as i32 * cell).max(0);"""
content = content.replace(old_obstacle, new_obstacle)


# 2. Add the Hologram Box right before drawing text
text_start = """        // --- 绘制中心发光文字 "无限光河" 作为物理障碍的可视化 ---"""
hologram = """        // --- 绘制全息玻璃容器边框 ---
        let box_x0 = 76;
        let box_y0 = 96;
        let box_x1 = 236;
        let box_y1 = 136;
        
        let c_glass: u16 = 0x0024; // 极幽暗的深蓝色
        let c_border: u16 = 0x03E0; // 暗青色边框
        let c_corner: u16 = 0x07ff; // 高亮青色四角
        
        for py in box_y0..box_y1 {
            for px in box_x0..box_x1 {
                if px >= 0 && px < fb_w && py >= 0 && py < fb_h {
                    let o = (py * fb_w + px) as usize;
                    
                    let is_corner_x = (px - box_x0 < 8) || (box_x1 - px <= 8);
                    let is_corner_y = (py - box_y0 < 8) || (box_y1 - py <= 8);
                    let is_edge_x = px == box_x0 || px == box_x1 - 1;
                    let is_edge_y = py == box_y0 || py == box_y1 - 1;
                    
                    if (is_edge_x && is_corner_y) || (is_edge_y && is_corner_x) {
                        buf[o] = c_corner;
                    } else if is_edge_x || is_edge_y {
                        // buf[o] = c_border; // 或者干脆不画长边框，只画四角，更高级！
                        if py % 4 == 0 || px % 4 == 0 {
                            buf[o] = c_border; // 虚线边框
                        }
                    } else {
                        // 内部区域如果没被水覆盖，则画玻璃底色
                        if buf[o] == 0 {
                            buf[o] = c_glass;
                        }
                    }
                }
            }
        }

        // --- 绘制中心发光文字 "无限光河" 作为物理障碍的可视化 ---"""
content = content.replace(text_start, hologram)

with open('src/render.rs', 'w') as f:
    f.write(content)
print("Done")
