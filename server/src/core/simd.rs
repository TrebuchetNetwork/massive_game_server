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

#[cfg(test)]
mod tests {
    use super::{filter_indices_within_radius, first_index_within_radius};

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
}
