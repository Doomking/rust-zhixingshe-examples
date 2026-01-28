use nalgebra::{Point3, Vector3};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[derive(Serialize, Deserialize)]
pub struct SliceResult {
    pub mesh_a: Vec<f32>, // 留在切面上方的顶点
    pub mesh_b: Vec<f32>, // 留在切面下方的顶点
}

pub struct Plane {
    pub normal: Vector3<f32>,
    pub point: Point3<f32>,
}

impl Plane {
    // 计算点到平面的有向距离：正数在上方，负数在下方
    fn distance_to(&self, p: Point3<f32>) -> f32 {
        self.normal.dot(&(p - self.point))
    }

    // 计算线段 A-B 与平面的交点 (线性插值)
    fn intersect(&self, a: Point3<f32>, b: Point3<f32>) -> Point3<f32> {
        let da = self.distance_to(a);
        let db = self.distance_to(b);
        let t = da / (da - db);
        a + t * (b - a)
    }
}

#[wasm_bindgen]
pub fn slice_mesh(
    vertices: &[f32], // 输入：原始顶点 Float32Array
    p_norm: &[f32],   // 输入：切面法线 [x, y, z]
    p_point: &[f32],  // 输入：切面上的一点 [x, y, z]
) -> JsValue {
    let plane = Plane {
        normal: Vector3::new(p_norm[0], p_norm[1], p_norm[2]),
        point: Point3::new(p_point[0], p_point[1], p_point[2]),
    };

    // 预分配内存，减少动态扩容
    let triangle_count = vertices.len() / 9;
    let mut mesh_a = Vec::with_capacity(triangle_count * 9);
    let mut mesh_b = Vec::with_capacity(triangle_count * 9);

    // 步长为 9：每 3 个浮点数是一个点，每 3 个点是一个三角形
    // 确保 i + 8 < vertices.len()，避免索引越界
    let max_i = vertices.len().saturating_sub(9);
    for i in (0..=max_i).step_by(9) {
        // 直接访问数组元素，避免中间变量
        let tri0 = Point3::new(vertices[i], vertices[i + 1], vertices[i + 2]);
        let tri1 = Point3::new(vertices[i + 3], vertices[i + 4], vertices[i + 5]);
        let tri2 = Point3::new(vertices[i + 6], vertices[i + 7], vertices[i + 8]);

        let dist0 = plane.distance_to(tri0);
        let dist1 = plane.distance_to(tri1);
        let dist2 = plane.distance_to(tri2);

        // 快速判断三角形是否完全在一侧
        if dist0 >= 0.0 && dist1 >= 0.0 && dist2 >= 0.0 {
            // 全部在上方
            add_triangle(&mut mesh_a, tri0, tri1, tri2);
            continue;
        } else if dist0 < 0.0 && dist1 < 0.0 && dist2 < 0.0 {
            // 全部在下方
            add_triangle(&mut mesh_b, tri0, tri1, tri2);
            continue;
        }

        // 判定三角形 3 个顶点的分布情况
        let mut front_count = 0;
        let mut back_count = 0;
        let mut front_indices = [0; 3];
        let mut back_indices = [0; 3];

        if dist0 >= 0.0 {
            front_indices[front_count] = 0;
            front_count += 1;
        } else {
            back_indices[back_count] = 0;
            back_count += 1;
        }

        if dist1 >= 0.0 {
            front_indices[front_count] = 1;
            front_count += 1;
        } else {
            back_indices[back_count] = 1;
            back_count += 1;
        }

        if dist2 >= 0.0 {
            front_indices[front_count] = 2;
            front_count += 1;
        } else {
            back_indices[back_count] = 2;
            back_count += 1;
        }

        match front_count {
            1 => {
                // 1个在上方，2个在下方 -> 上方分得1个三角形，下方分得1个四边形（拆为2个三角形）
                let f = front_indices[0];
                let b1 = back_indices[0];
                let b2 = back_indices[1];

                let tri = [tri0, tri1, tri2];
                let i1 = plane.intersect(tri[f], tri[b1]);
                let i2 = plane.intersect(tri[f], tri[b2]);

                add_triangle(&mut mesh_a, tri[f], i1, i2);
                add_triangle(&mut mesh_b, i1, tri[b1], tri[b2]);
                add_triangle(&mut mesh_b, i1, tri[b2], i2);
            }
            2 => {
                // 2个在上方，1个在下方 -> 下方分得1个三角形，上方分得1个四边形（拆为2个三角形）
                let b = back_indices[0];
                let f1 = front_indices[0];
                let f2 = front_indices[1];

                let tri = [tri0, tri1, tri2];
                let i1 = plane.intersect(tri[b], tri[f1]);
                let i2 = plane.intersect(tri[b], tri[f2]);

                add_triangle(&mut mesh_b, tri[b], i1, i2);
                add_triangle(&mut mesh_a, i1, tri[f1], tri[f2]);
                add_triangle(&mut mesh_a, i1, tri[f2], i2);
            }
            _ => {
                // 已经在上面处理过了
            }
        }
    }

    let result = SliceResult { mesh_a, mesh_b };
    serde_wasm_bindgen::to_value(&result).unwrap()
}

// 辅助函数：将三角形顶点平铺到 Vec 中
fn add_triangle(v_list: &mut Vec<f32>, p1: Point3<f32>, p2: Point3<f32>, p3: Point3<f32>) {
    v_list.extend_from_slice(&[p1.x, p1.y, p1.z, p2.x, p2.y, p2.z, p3.x, p3.y, p3.z]);
}
