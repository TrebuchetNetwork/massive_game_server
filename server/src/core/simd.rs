// massive_game_server/server/src/core/simd.rs
//
// SIMD helpers for high-frequency spatial and collision checks with scalar fallback.

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64 as neon_arch;
#[cfg(target_arch = "x86")]
use std::arch::x86 as x86_arch;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64 as x86_arch;

#[inline]
pub fn filter_indices_within_radius(
    xs: &[f32],
    ys: &[f32],
    center_x: f32,
    center_y: f32,
    radius_squared: f32,
    out_indices: &mut Vec<usize>,
) {
    out_indices.clear();

    if xs.len() != ys.len() || xs.is_empty() {
        return;
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avx2") {
            unsafe {
                filter_indices_within_radius_avx2(
                    xs,
                    ys,
                    center_x,
                    center_y,
                    radius_squared,
                    out_indices,
                );
            }
            return;
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            unsafe {
                filter_indices_within_radius_neon(
                    xs,
                    ys,
                    center_x,
                    center_y,
                    radius_squared,
                    out_indices,
                );
            }
            return;
        }
    }

    filter_indices_within_radius_scalar(xs, ys, center_x, center_y, radius_squared, out_indices);
}

#[inline]
pub fn first_index_within_radius(
    xs: &[f32],
    ys: &[f32],
    center_x: f32,
    center_y: f32,
    radius_squared: f32,
) -> Option<usize> {
    if xs.len() != ys.len() || xs.is_empty() {
        return None;
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avx2") {
            return unsafe {
                first_index_within_radius_avx2(xs, ys, center_x, center_y, radius_squared)
            };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            return unsafe {
                first_index_within_radius_neon(xs, ys, center_x, center_y, radius_squared)
            };
        }
    }

    first_index_within_radius_scalar(xs, ys, center_x, center_y, radius_squared)
}

#[inline]
pub fn first_index_within_segment_radius(
    xs: &[f32],
    ys: &[f32],
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    radius_squared: f32,
) -> Option<usize> {
    if xs.len() != ys.len() || xs.is_empty() {
        return None;
    }

    let seg_dx = end_x - start_x;
    let seg_dy = end_y - start_y;
    let seg_len_sq = seg_dx * seg_dx + seg_dy * seg_dy;
    if seg_len_sq <= f32::EPSILON {
        return first_index_within_radius(xs, ys, start_x, start_y, radius_squared);
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avx2") {
            return unsafe {
                first_index_within_segment_radius_avx2(
                    xs,
                    ys,
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    radius_squared,
                )
            };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            return unsafe {
                first_index_within_segment_radius_neon(
                    xs,
                    ys,
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    radius_squared,
                )
            };
        }
    }

    first_index_within_segment_radius_scalar(xs, ys, start_x, start_y, end_x, end_y, radius_squared)
}

#[inline]
pub fn first_index_aabb_containing_point(
    min_xs: &[f32],
    max_xs: &[f32],
    min_ys: &[f32],
    max_ys: &[f32],
    point_x: f32,
    point_y: f32,
) -> Option<usize> {
    let len = min_xs.len();
    if len == 0 || max_xs.len() != len || min_ys.len() != len || max_ys.len() != len {
        return None;
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("avx2") {
            return unsafe {
                first_index_aabb_containing_point_avx2(
                    min_xs, max_xs, min_ys, max_ys, point_x, point_y,
                )
            };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon") {
            return unsafe {
                first_index_aabb_containing_point_neon(
                    min_xs, max_xs, min_ys, max_ys, point_x, point_y,
                )
            };
        }
    }

    first_index_aabb_containing_point_scalar(min_xs, max_xs, min_ys, max_ys, point_x, point_y)
}

#[inline]
fn filter_indices_within_radius_scalar(
    xs: &[f32],
    ys: &[f32],
    center_x: f32,
    center_y: f32,
    radius_squared: f32,
    out_indices: &mut Vec<usize>,
) {
    for idx in 0..xs.len() {
        let dx = xs[idx] - center_x;
        let dy = ys[idx] - center_y;
        if dx * dx + dy * dy <= radius_squared {
            out_indices.push(idx);
        }
    }
}

#[inline]
fn first_index_within_radius_scalar(
    xs: &[f32],
    ys: &[f32],
    center_x: f32,
    center_y: f32,
    radius_squared: f32,
) -> Option<usize> {
    for idx in 0..xs.len() {
        let dx = xs[idx] - center_x;
        let dy = ys[idx] - center_y;
        if dx * dx + dy * dy <= radius_squared {
            return Some(idx);
        }
    }
    None
}

#[inline]
fn first_index_within_segment_radius_scalar(
    xs: &[f32],
    ys: &[f32],
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    radius_squared: f32,
) -> Option<usize> {
    let seg_dx = end_x - start_x;
    let seg_dy = end_y - start_y;
    let seg_len_sq = seg_dx * seg_dx + seg_dy * seg_dy;
    if seg_len_sq <= f32::EPSILON {
        return first_index_within_radius_scalar(xs, ys, start_x, start_y, radius_squared);
    }

    for idx in 0..xs.len() {
        let px = xs[idx] - start_x;
        let py = ys[idx] - start_y;
        let t = ((px * seg_dx + py * seg_dy) / seg_len_sq).clamp(0.0, 1.0);
        let closest_x = start_x + t * seg_dx;
        let closest_y = start_y + t * seg_dy;
        let dx = xs[idx] - closest_x;
        let dy = ys[idx] - closest_y;
        if dx * dx + dy * dy <= radius_squared {
            return Some(idx);
        }
    }
    None
}

#[inline]
fn first_index_aabb_containing_point_scalar(
    min_xs: &[f32],
    max_xs: &[f32],
    min_ys: &[f32],
    max_ys: &[f32],
    point_x: f32,
    point_y: f32,
) -> Option<usize> {
    for idx in 0..min_xs.len() {
        if point_x >= min_xs[idx]
            && point_x <= max_xs[idx]
            && point_y >= min_ys[idx]
            && point_y <= max_ys[idx]
        {
            return Some(idx);
        }
    }
    None
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn filter_indices_within_radius_avx2(
    xs: &[f32],
    ys: &[f32],
    center_x: f32,
    center_y: f32,
    radius_squared: f32,
    out_indices: &mut Vec<usize>,
) {
    let center_x_v = x86_arch::_mm256_set1_ps(center_x);
    let center_y_v = x86_arch::_mm256_set1_ps(center_y);
    let radius_v = x86_arch::_mm256_set1_ps(radius_squared);

    let mut idx = 0usize;
    while idx + 8 <= xs.len() {
        let x_v = x86_arch::_mm256_loadu_ps(xs.as_ptr().add(idx));
        let y_v = x86_arch::_mm256_loadu_ps(ys.as_ptr().add(idx));

        let dx_v = x86_arch::_mm256_sub_ps(x_v, center_x_v);
        let dy_v = x86_arch::_mm256_sub_ps(y_v, center_y_v);
        let dist_sq_v = x86_arch::_mm256_add_ps(
            x86_arch::_mm256_mul_ps(dx_v, dx_v),
            x86_arch::_mm256_mul_ps(dy_v, dy_v),
        );

        let cmp_v = x86_arch::_mm256_cmp_ps(dist_sq_v, radius_v, x86_arch::_CMP_LE_OQ);
        let mut mask = x86_arch::_mm256_movemask_ps(cmp_v) as u32;
        while mask != 0 {
            let lane = mask.trailing_zeros() as usize;
            out_indices.push(idx + lane);
            mask &= mask - 1;
        }
        idx += 8;
    }

    for tail_idx in idx..xs.len() {
        let dx = xs[tail_idx] - center_x;
        let dy = ys[tail_idx] - center_y;
        if dx * dx + dy * dy <= radius_squared {
            out_indices.push(tail_idx);
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn first_index_within_radius_avx2(
    xs: &[f32],
    ys: &[f32],
    center_x: f32,
    center_y: f32,
    radius_squared: f32,
) -> Option<usize> {
    let center_x_v = x86_arch::_mm256_set1_ps(center_x);
    let center_y_v = x86_arch::_mm256_set1_ps(center_y);
    let radius_v = x86_arch::_mm256_set1_ps(radius_squared);

    let mut idx = 0usize;
    while idx + 8 <= xs.len() {
        let x_v = x86_arch::_mm256_loadu_ps(xs.as_ptr().add(idx));
        let y_v = x86_arch::_mm256_loadu_ps(ys.as_ptr().add(idx));

        let dx_v = x86_arch::_mm256_sub_ps(x_v, center_x_v);
        let dy_v = x86_arch::_mm256_sub_ps(y_v, center_y_v);
        let dist_sq_v = x86_arch::_mm256_add_ps(
            x86_arch::_mm256_mul_ps(dx_v, dx_v),
            x86_arch::_mm256_mul_ps(dy_v, dy_v),
        );

        let cmp_v = x86_arch::_mm256_cmp_ps(dist_sq_v, radius_v, x86_arch::_CMP_LE_OQ);
        let mask = x86_arch::_mm256_movemask_ps(cmp_v) as u32;
        if mask != 0 {
            return Some(idx + mask.trailing_zeros() as usize);
        }
        idx += 8;
    }

    for tail_idx in idx..xs.len() {
        let dx = xs[tail_idx] - center_x;
        let dy = ys[tail_idx] - center_y;
        if dx * dx + dy * dy <= radius_squared {
            return Some(tail_idx);
        }
    }
    None
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn first_index_within_segment_radius_avx2(
    xs: &[f32],
    ys: &[f32],
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    radius_squared: f32,
) -> Option<usize> {
    let seg_dx = end_x - start_x;
    let seg_dy = end_y - start_y;
    let seg_len_sq = seg_dx * seg_dx + seg_dy * seg_dy;
    if seg_len_sq <= f32::EPSILON {
        return first_index_within_radius_scalar(xs, ys, start_x, start_y, radius_squared);
    }

    let inv_seg_len_sq = 1.0 / seg_len_sq;
    let start_x_v = x86_arch::_mm256_set1_ps(start_x);
    let start_y_v = x86_arch::_mm256_set1_ps(start_y);
    let seg_dx_v = x86_arch::_mm256_set1_ps(seg_dx);
    let seg_dy_v = x86_arch::_mm256_set1_ps(seg_dy);
    let inv_len_v = x86_arch::_mm256_set1_ps(inv_seg_len_sq);
    let zero_v = x86_arch::_mm256_set1_ps(0.0);
    let one_v = x86_arch::_mm256_set1_ps(1.0);
    let radius_v = x86_arch::_mm256_set1_ps(radius_squared);

    let mut idx = 0usize;
    while idx + 8 <= xs.len() {
        let x_v = x86_arch::_mm256_loadu_ps(xs.as_ptr().add(idx));
        let y_v = x86_arch::_mm256_loadu_ps(ys.as_ptr().add(idx));

        let px_v = x86_arch::_mm256_sub_ps(x_v, start_x_v);
        let py_v = x86_arch::_mm256_sub_ps(y_v, start_y_v);
        let dot_v = x86_arch::_mm256_add_ps(
            x86_arch::_mm256_mul_ps(px_v, seg_dx_v),
            x86_arch::_mm256_mul_ps(py_v, seg_dy_v),
        );
        let t_unclamped = x86_arch::_mm256_mul_ps(dot_v, inv_len_v);
        let t_v = x86_arch::_mm256_max_ps(zero_v, x86_arch::_mm256_min_ps(one_v, t_unclamped));

        let closest_x_v =
            x86_arch::_mm256_add_ps(start_x_v, x86_arch::_mm256_mul_ps(t_v, seg_dx_v));
        let closest_y_v =
            x86_arch::_mm256_add_ps(start_y_v, x86_arch::_mm256_mul_ps(t_v, seg_dy_v));
        let dx_v = x86_arch::_mm256_sub_ps(x_v, closest_x_v);
        let dy_v = x86_arch::_mm256_sub_ps(y_v, closest_y_v);
        let dist_sq_v = x86_arch::_mm256_add_ps(
            x86_arch::_mm256_mul_ps(dx_v, dx_v),
            x86_arch::_mm256_mul_ps(dy_v, dy_v),
        );

        let cmp_v = x86_arch::_mm256_cmp_ps(dist_sq_v, radius_v, x86_arch::_CMP_LE_OQ);
        let mask = x86_arch::_mm256_movemask_ps(cmp_v) as u32;
        if mask != 0 {
            return Some(idx + mask.trailing_zeros() as usize);
        }
        idx += 8;
    }

    for tail_idx in idx..xs.len() {
        let px = xs[tail_idx] - start_x;
        let py = ys[tail_idx] - start_y;
        let t = ((px * seg_dx + py * seg_dy) * inv_seg_len_sq).clamp(0.0, 1.0);
        let closest_x = start_x + t * seg_dx;
        let closest_y = start_y + t * seg_dy;
        let dx = xs[tail_idx] - closest_x;
        let dy = ys[tail_idx] - closest_y;
        if dx * dx + dy * dy <= radius_squared {
            return Some(tail_idx);
        }
    }
    None
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn first_index_within_segment_radius_neon(
    xs: &[f32],
    ys: &[f32],
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    radius_squared: f32,
) -> Option<usize> {
    let seg_dx = end_x - start_x;
    let seg_dy = end_y - start_y;
    let seg_len_sq = seg_dx * seg_dx + seg_dy * seg_dy;
    if seg_len_sq <= f32::EPSILON {
        return first_index_within_radius_scalar(xs, ys, start_x, start_y, radius_squared);
    }

    let inv_seg_len_sq = 1.0 / seg_len_sq;
    let start_x_v = neon_arch::vdupq_n_f32(start_x);
    let start_y_v = neon_arch::vdupq_n_f32(start_y);
    let seg_dx_v = neon_arch::vdupq_n_f32(seg_dx);
    let seg_dy_v = neon_arch::vdupq_n_f32(seg_dy);
    let inv_len_v = neon_arch::vdupq_n_f32(inv_seg_len_sq);
    let zero_v = neon_arch::vdupq_n_f32(0.0);
    let one_v = neon_arch::vdupq_n_f32(1.0);
    let radius_v = neon_arch::vdupq_n_f32(radius_squared);

    let mut idx = 0usize;
    while idx + 4 <= xs.len() {
        let x_v = neon_arch::vld1q_f32(xs.as_ptr().add(idx));
        let y_v = neon_arch::vld1q_f32(ys.as_ptr().add(idx));

        let px_v = neon_arch::vsubq_f32(x_v, start_x_v);
        let py_v = neon_arch::vsubq_f32(y_v, start_y_v);
        let dot_v = neon_arch::vaddq_f32(
            neon_arch::vmulq_f32(px_v, seg_dx_v),
            neon_arch::vmulq_f32(py_v, seg_dy_v),
        );
        let t_unclamped_v = neon_arch::vmulq_f32(dot_v, inv_len_v);
        let t_v = neon_arch::vmaxq_f32(zero_v, neon_arch::vminq_f32(one_v, t_unclamped_v));

        let closest_x_v = neon_arch::vaddq_f32(start_x_v, neon_arch::vmulq_f32(t_v, seg_dx_v));
        let closest_y_v = neon_arch::vaddq_f32(start_y_v, neon_arch::vmulq_f32(t_v, seg_dy_v));
        let dx_v = neon_arch::vsubq_f32(x_v, closest_x_v);
        let dy_v = neon_arch::vsubq_f32(y_v, closest_y_v);
        let dist_sq_v = neon_arch::vaddq_f32(
            neon_arch::vmulq_f32(dx_v, dx_v),
            neon_arch::vmulq_f32(dy_v, dy_v),
        );

        let cmp_v = neon_arch::vcleq_f32(dist_sq_v, radius_v);
        let mut cmp_arr = [0u32; 4];
        neon_arch::vst1q_u32(cmp_arr.as_mut_ptr(), cmp_v);
        for lane in 0..4usize {
            if cmp_arr[lane] != 0 {
                return Some(idx + lane);
            }
        }
        idx += 4;
    }

    for tail_idx in idx..xs.len() {
        let px = xs[tail_idx] - start_x;
        let py = ys[tail_idx] - start_y;
        let t = ((px * seg_dx + py * seg_dy) * inv_seg_len_sq).clamp(0.0, 1.0);
        let closest_x = start_x + t * seg_dx;
        let closest_y = start_y + t * seg_dy;
        let dx = xs[tail_idx] - closest_x;
        let dy = ys[tail_idx] - closest_y;
        if dx * dx + dy * dy <= radius_squared {
            return Some(tail_idx);
        }
    }

    None
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn first_index_aabb_containing_point_avx2(
    min_xs: &[f32],
    max_xs: &[f32],
    min_ys: &[f32],
    max_ys: &[f32],
    point_x: f32,
    point_y: f32,
) -> Option<usize> {
    let point_x_v = x86_arch::_mm256_set1_ps(point_x);
    let point_y_v = x86_arch::_mm256_set1_ps(point_y);

    let mut idx = 0usize;
    while idx + 8 <= min_xs.len() {
        let min_x_v = x86_arch::_mm256_loadu_ps(min_xs.as_ptr().add(idx));
        let max_x_v = x86_arch::_mm256_loadu_ps(max_xs.as_ptr().add(idx));
        let min_y_v = x86_arch::_mm256_loadu_ps(min_ys.as_ptr().add(idx));
        let max_y_v = x86_arch::_mm256_loadu_ps(max_ys.as_ptr().add(idx));

        let ge_min_x = x86_arch::_mm256_cmp_ps(point_x_v, min_x_v, x86_arch::_CMP_GE_OQ);
        let le_max_x = x86_arch::_mm256_cmp_ps(point_x_v, max_x_v, x86_arch::_CMP_LE_OQ);
        let ge_min_y = x86_arch::_mm256_cmp_ps(point_y_v, min_y_v, x86_arch::_CMP_GE_OQ);
        let le_max_y = x86_arch::_mm256_cmp_ps(point_y_v, max_y_v, x86_arch::_CMP_LE_OQ);

        let inside_x = x86_arch::_mm256_and_ps(ge_min_x, le_max_x);
        let inside_y = x86_arch::_mm256_and_ps(ge_min_y, le_max_y);
        let inside = x86_arch::_mm256_and_ps(inside_x, inside_y);

        let mask = x86_arch::_mm256_movemask_ps(inside) as u32;
        if mask != 0 {
            return Some(idx + mask.trailing_zeros() as usize);
        }
        idx += 8;
    }

    for tail_idx in idx..min_xs.len() {
        if point_x >= min_xs[tail_idx]
            && point_x <= max_xs[tail_idx]
            && point_y >= min_ys[tail_idx]
            && point_y <= max_ys[tail_idx]
        {
            return Some(tail_idx);
        }
    }
    None
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn filter_indices_within_radius_neon(
    xs: &[f32],
    ys: &[f32],
    center_x: f32,
    center_y: f32,
    radius_squared: f32,
    out_indices: &mut Vec<usize>,
) {
    let center_x_v = neon_arch::vdupq_n_f32(center_x);
    let center_y_v = neon_arch::vdupq_n_f32(center_y);
    let radius_v = neon_arch::vdupq_n_f32(radius_squared);

    let mut idx = 0usize;
    while idx + 4 <= xs.len() {
        let x_v = neon_arch::vld1q_f32(xs.as_ptr().add(idx));
        let y_v = neon_arch::vld1q_f32(ys.as_ptr().add(idx));

        let dx_v = neon_arch::vsubq_f32(x_v, center_x_v);
        let dy_v = neon_arch::vsubq_f32(y_v, center_y_v);
        let dist_sq_v = neon_arch::vaddq_f32(
            neon_arch::vmulq_f32(dx_v, dx_v),
            neon_arch::vmulq_f32(dy_v, dy_v),
        );

        let cmp_v = neon_arch::vcleq_f32(dist_sq_v, radius_v);
        let mut cmp_arr = [0u32; 4];
        neon_arch::vst1q_u32(cmp_arr.as_mut_ptr(), cmp_v);
        for lane in 0..4usize {
            if cmp_arr[lane] != 0 {
                out_indices.push(idx + lane);
            }
        }
        idx += 4;
    }

    for tail_idx in idx..xs.len() {
        let dx = xs[tail_idx] - center_x;
        let dy = ys[tail_idx] - center_y;
        if dx * dx + dy * dy <= radius_squared {
            out_indices.push(tail_idx);
        }
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn first_index_within_radius_neon(
    xs: &[f32],
    ys: &[f32],
    center_x: f32,
    center_y: f32,
    radius_squared: f32,
) -> Option<usize> {
    let center_x_v = neon_arch::vdupq_n_f32(center_x);
    let center_y_v = neon_arch::vdupq_n_f32(center_y);
    let radius_v = neon_arch::vdupq_n_f32(radius_squared);

    let mut idx = 0usize;
    while idx + 4 <= xs.len() {
        let x_v = neon_arch::vld1q_f32(xs.as_ptr().add(idx));
        let y_v = neon_arch::vld1q_f32(ys.as_ptr().add(idx));

        let dx_v = neon_arch::vsubq_f32(x_v, center_x_v);
        let dy_v = neon_arch::vsubq_f32(y_v, center_y_v);
        let dist_sq_v = neon_arch::vaddq_f32(
            neon_arch::vmulq_f32(dx_v, dx_v),
            neon_arch::vmulq_f32(dy_v, dy_v),
        );

        let cmp_v = neon_arch::vcleq_f32(dist_sq_v, radius_v);
        let mut cmp_arr = [0u32; 4];
        neon_arch::vst1q_u32(cmp_arr.as_mut_ptr(), cmp_v);
        for lane in 0..4usize {
            if cmp_arr[lane] != 0 {
                return Some(idx + lane);
            }
        }
        idx += 4;
    }

    for tail_idx in idx..xs.len() {
        let dx = xs[tail_idx] - center_x;
        let dy = ys[tail_idx] - center_y;
        if dx * dx + dy * dy <= radius_squared {
            return Some(tail_idx);
        }
    }
    None
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn first_index_aabb_containing_point_neon(
    min_xs: &[f32],
    max_xs: &[f32],
    min_ys: &[f32],
    max_ys: &[f32],
    point_x: f32,
    point_y: f32,
) -> Option<usize> {
    let point_x_v = neon_arch::vdupq_n_f32(point_x);
    let point_y_v = neon_arch::vdupq_n_f32(point_y);

    let mut idx = 0usize;
    while idx + 4 <= min_xs.len() {
        let min_x_v = neon_arch::vld1q_f32(min_xs.as_ptr().add(idx));
        let max_x_v = neon_arch::vld1q_f32(max_xs.as_ptr().add(idx));
        let min_y_v = neon_arch::vld1q_f32(min_ys.as_ptr().add(idx));
        let max_y_v = neon_arch::vld1q_f32(max_ys.as_ptr().add(idx));

        let ge_min_x = neon_arch::vcgeq_f32(point_x_v, min_x_v);
        let le_max_x = neon_arch::vcleq_f32(point_x_v, max_x_v);
        let ge_min_y = neon_arch::vcgeq_f32(point_y_v, min_y_v);
        let le_max_y = neon_arch::vcleq_f32(point_y_v, max_y_v);

        let inside_x = neon_arch::vandq_u32(ge_min_x, le_max_x);
        let inside_y = neon_arch::vandq_u32(ge_min_y, le_max_y);
        let inside = neon_arch::vandq_u32(inside_x, inside_y);

        let mut cmp_arr = [0u32; 4];
        neon_arch::vst1q_u32(cmp_arr.as_mut_ptr(), inside);
        for lane in 0..4usize {
            if cmp_arr[lane] != 0 {
                return Some(idx + lane);
            }
        }
        idx += 4;
    }

    for tail_idx in idx..min_xs.len() {
        if point_x >= min_xs[tail_idx]
            && point_x <= max_xs[tail_idx]
            && point_y >= min_ys[tail_idx]
            && point_y <= max_ys[tail_idx]
        {
            return Some(tail_idx);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        filter_indices_within_radius, first_index_aabb_containing_point, first_index_within_radius,
        first_index_within_segment_radius,
    };

    #[test]
    fn simd_filter_matches_expected_indices() {
        let xs = [0.0, 2.0, 3.0, 9.0, -1.0];
        let ys = [0.0, 2.0, 0.0, 0.0, -1.0];
        let mut matches = Vec::new();
        filter_indices_within_radius(&xs, &ys, 0.0, 0.0, 9.0, &mut matches);
        assert_eq!(matches, vec![0, 1, 2, 4]);
    }

    #[test]
    fn simd_first_index_returns_first_match() {
        let xs = [8.0, 7.0, 1.0, 4.0];
        let ys = [8.0, 7.0, 1.0, 4.0];
        let idx = first_index_within_radius(&xs, &ys, 0.0, 0.0, 5.0);
        assert_eq!(idx, Some(2));
    }

    #[test]
    fn simd_first_index_aabb_returns_first_match() {
        let min_xs = [10.0, -5.0, 0.0];
        let max_xs = [20.0, -1.0, 2.0];
        let min_ys = [10.0, -5.0, 0.0];
        let max_ys = [20.0, -1.0, 2.0];
        let idx = first_index_aabb_containing_point(&min_xs, &max_xs, &min_ys, &max_ys, 1.5, 1.5);
        assert_eq!(idx, Some(2));
    }

    #[test]
    fn simd_segment_radius_returns_first_hit_along_path() {
        let xs = [12.0, 5.0, 2.0, -3.0];
        let ys = [2.0, 0.5, 0.0, 0.0];
        let idx = first_index_within_segment_radius(&xs, &ys, 0.0, 0.0, 10.0, 0.0, 1.0);
        assert_eq!(idx, Some(1));
    }

    #[test]
    fn simd_segment_radius_handles_zero_length_segment() {
        let xs = [10.0, 0.5, 2.0];
        let ys = [10.0, 0.5, 2.0];
        let idx = first_index_within_segment_radius(&xs, &ys, 0.0, 0.0, 0.0, 0.0, 0.75);
        assert_eq!(idx, Some(1));
    }
}
