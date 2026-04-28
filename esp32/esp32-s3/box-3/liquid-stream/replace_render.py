with open('src/render.rs', 'r') as f:
    content = f.read()

old_rect = """        // 绘制物理网格背景 (比如障碍物)
        for y in 0..fluid.ny {
            for x in 0..fluid.nx {
                if fluid.is_solid(x, y) {
                    let rect = Rectangle::new(
                        Point::new((x * 10) as i32, (y * 10) as i32),
                        Size::new(10, 10),
                    );
                    let _ = rect.draw(&mut PrimitiveStyle::with_fill(C_OBSTACLE), buf);
                }
            }
        }"""
new_rect = """        // 绘制物理网格背景 (比如障碍物)
        for y in 0..fluid.ny {
            for x in 0..fluid.nx {
                if fluid.is_solid(x, y) {
                    // 隐藏用于文字碰撞的内部宏观网格
                    if y >= 10 && y <= 13 && x >= 8 && x <= 23 {
                        continue;
                    }
                    let rect = Rectangle::new(
                        Point::new((x * 10) as i32, (y * 10) as i32),
                        Size::new(10, 10),
                    );
                    let _ = rect.draw(&mut PrimitiveStyle::with_fill(C_OBSTACLE), buf);
                }
            }
        }"""
content = content.replace(old_rect, new_rect)

with open('src/render.rs', 'w') as f:
    f.write(content)
print("Done")
