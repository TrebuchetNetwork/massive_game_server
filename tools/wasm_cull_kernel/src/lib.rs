#[inline(always)]
fn in_view(x: f32, y: f32, left: f32, right: f32, top: f32, bottom: f32, margin: f32) -> bool {
    x >= (left - margin) && x <= (right + margin) && y >= (top - margin) && y <= (bottom + margin)
}

#[inline(always)]
fn distance_sq(x: f32, y: f32, anchor_x: f32, anchor_y: f32) -> f32 {
    let dx = x - anchor_x;
    let dy = y - anchor_y;
    (dx * dx) + (dy * dy)
}

#[no_mangle]
pub extern "C" fn cull_visibility(
    x: f32,
    y: f32,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    margin: f32,
) -> i32 {
    if in_view(x, y, left, right, top, bottom, margin) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn cull_distance_sq(
    x: f32,
    y: f32,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    margin: f32,
    anchor_x: f32,
    anchor_y: f32,
) -> f32 {
    if !in_view(x, y, left, right, top, bottom, margin) {
        return -1.0;
    }
    distance_sq(x, y, anchor_x, anchor_y)
}

#[no_mangle]
pub extern "C" fn kernel_version() -> i32 {
    1
}

