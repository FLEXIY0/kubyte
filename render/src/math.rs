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

fn translate([x, y, z]: [f32; 3]) -> Mat4 {
    let mut m = IDENT;
    m[3] = [x, y, z, 1.0];
    m
}

fn scale3([x, y, z]: [f32; 3]) -> Mat4 {
    let mut m = IDENT;
    (m[0][0], m[1][1], m[2][2]) = (x, y, z);
    m
}

fn rot_y(a: f32) -> Mat4 {
    let (s, c) = a.sin_cos();
    let mut m = IDENT;
    (m[0][0], m[0][2], m[2][0], m[2][2]) = (c, -s, s, c);
    m
}

fn rot_x(a: f32) -> Mat4 {
    let (s, c) = a.sin_cos();
    let mut m = IDENT;
    (m[1][1], m[1][2], m[2][1], m[2][2]) = (c, s, -s, c);
    m
}

const IDENT: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// Неподвижная часть тела моба: куб [-0.5..0.5]³ → размер `size`,
/// центр в `center` (локально, над ступнями), разворот по курсу.
pub fn mob_part(pos: [f32; 3], yaw: f32, size: [f32; 3], center: [f32; 3]) -> Mat4 {
    mul(
        &mul(&translate(pos), &rot_y(yaw)),
        &mul(&translate(center), &scale3(size)),
    )
}

/// Конечность: крепится суставом в `joint` (локально, над ступнями),
/// свисает вниз на свою длину и качается вокруг сустава на `swing` радиан.
pub fn mob_limb(pos: [f32; 3], yaw: f32, size: [f32; 3], joint: [f32; 3], swing: f32) -> Mat4 {
    let hang = mul(&translate([0.0, -size[1] / 2.0, 0.0]), &scale3(size));
    let jointed = mul(&mul(&translate(joint), &rot_x(swing)), &hang);
    mul(&mul(&translate(pos), &rot_y(yaw)), &jointed)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Конечность без качания — это просто часть, висящая под суставом:
    /// два пути построения матрицы обязаны сойтись.
    #[test]
    fn limb_at_rest_equals_static_part() {
        let (pos, yaw, size, joint) = ([3.0, 64.0, -2.0], 0.8, [0.25, 0.75, 0.25], [0.125, 0.75, 0.0]);
        let limb = mob_limb(pos, yaw, size, joint, 0.0);
        let part = mob_part(pos, yaw, size, [joint[0], joint[1] - size[1] / 2.0, joint[2]]);
        for c in 0..4 {
            for r in 0..4 {
                assert!((limb[c][r] - part[c][r]).abs() < 1e-5, "[{c}][{r}]");
            }
        }
    }
}
