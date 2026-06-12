//! Минимальная матричная математика для камеры и отсечения.
//!
//! Своя, а не glam: нужно пять функций, и 80 строк кода дешевле
//! зависимости (§2). Рендер — единственное место проекта, где float
//! разрешён: он чисто визуален и в детерминизм мира не входит (§6).

/// Колоночно-мажорная 4×4, как её ждёт WGSL.
pub type Mat4 = [[f32; 4]; 4];

pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [[0.0; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            m[c][r] = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    m
}

/// Перспектива right-handed, глубина 0..1 (соглашение wgpu).
pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y / 2.0).tan();
    let k = far / (near - far);
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, k, -1.0],
        [0.0, 0.0, near * k, 0.0],
    ]
}

/// Матрица вида FPS-камеры: перенос в начало координат, разворот по yaw,
/// затем по pitch. Раскрыта вручную — три умножения 4×4 здесь не нужны.
pub fn view(pos: [f32; 3], yaw: f32, pitch: f32) -> Mat4 {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    // Базис камеры в мировых осях (right, up, back).
    let r = [cy, 0.0, -sy];
    let u = [sy * sp, cp, cy * sp];
    let b = [sy * cp, -sp, cy * cp];
    let dot = |a: [f32; 3]| -(a[0] * pos[0] + a[1] * pos[1] + a[2] * pos[2]);
    [
        [r[0], u[0], b[0], 0.0],
        [r[1], u[1], b[1], 0.0],
        [r[2], u[2], b[2], 0.0],
        [dot(r), dot(u), dot(b), 1.0],
    ]
}

/// Матрица части моба: куб [-0.5..0.5]³ → размер `size`, поворот по yaw,
/// центр на высоте `lift` над ступнями, смещение `fwd` по взгляду.
pub fn mob_part(pos: [f32; 3], yaw: f32, size: [f32; 3], lift: f32, fwd: f32) -> Mat4 {
    let (s, c) = yaw.sin_cos();
    [
        [c * size[0], 0.0, -s * size[0], 0.0],
        [0.0, size[1], 0.0, 0.0],
        [s * size[2], 0.0, c * size[2], 0.0],
        [pos[0] + s * fwd, pos[1] + lift, pos[2] + c * fwd, 1.0],
    ]
}

/// Шесть плоскостей фрустума из матрицы view-projection (метод
/// Грибба—Хартманна, вариант для глубины 0..1). Плоскость — (a,b,c,d),
/// точка видима при ax+by+cz+d ≥ 0.
pub fn frustum(m: &Mat4) -> [[f32; 4]; 6] {
    let row = |i: usize| [m[0][i], m[1][i], m[2][i], m[3][i]];
    let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
    let add = |a: [f32; 4], b: [f32; 4], s: f32| {
        [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2], a[3] + s * b[3]]
    };
    [
        add(r3, r0, 1.0),  // left
        add(r3, r0, -1.0), // right
        add(r3, r1, 1.0),  // bottom
        add(r3, r1, -1.0), // top
        r2,                // near (z ≥ 0)
        add(r3, r2, -1.0), // far
    ]
}

/// AABB против фрустума: для каждой плоскости берём ближайшую к ней
/// «положительную» вершину коробки; если и она снаружи — коробка не видна.
pub fn aabb_visible(planes: &[[f32; 4]; 6], min: [f32; 3], max: [f32; 3]) -> bool {
    planes.iter().all(|p| {
        let v = |i: usize| if p[i] >= 0.0 { max[i] } else { min[i] };
        p[0] * v(0) + p[1] * v(1) + p[2] * v(2) + p[3] >= 0.0
    })
}
