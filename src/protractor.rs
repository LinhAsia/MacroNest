pub(crate) const PROTRACTOR_BASE_RADIUS: f32 = 150.0;
pub(crate) const PROTRACTOR_MIN_SCALE: f32 = 0.4;
pub(crate) const PROTRACTOR_MAX_SCALE: f32 = 2.5;
pub(crate) const PROTRACTOR_MIN_CALIBRATION_RADIUS: f32 =
    PROTRACTOR_BASE_RADIUS * PROTRACTOR_MIN_SCALE;

pub(crate) fn calibrated_protractor_scale(radius: f32) -> f32 {
    (radius / PROTRACTOR_BASE_RADIUS).clamp(PROTRACTOR_MIN_SCALE, PROTRACTOR_MAX_SCALE)
}

pub(crate) fn circle_from_3_points(
    p1: (i32, i32),
    p2: (i32, i32),
    p3: (i32, i32),
) -> Option<((i32, i32), f32)> {
    let (x1, y1) = (p1.0 as f64, p1.1 as f64);
    let (x2, y2) = (p2.0 as f64, p2.1 as f64);
    let (x3, y3) = (p3.0 as f64, p3.1 as f64);

    let d = 2.0 * (x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2));
    if d.abs() < 1e-6 {
        return None;
    }

    let ux = ((x1 * x1 + y1 * y1) * (y2 - y3)
        + (x2 * x2 + y2 * y2) * (y3 - y1)
        + (x3 * x3 + y3 * y3) * (y1 - y2))
        / d;
    let uy = ((x1 * x1 + y1 * y1) * (x3 - x2)
        + (x2 * x2 + y2 * y2) * (x1 - x3)
        + (x3 * x3 + y3 * y3) * (x2 - x1))
        / d;

    let radius = ((x1 - ux).powi(2) + (y1 - uy).powi(2)).sqrt();
    Some(((ux.round() as i32, uy.round() as i32), radius as f32))
}

#[cfg(test)]
mod tests {
    use super::{
        PROTRACTOR_MIN_CALIBRATION_RADIUS, calibrated_protractor_scale, circle_from_3_points,
    };

    #[test]
    fn circle_from_three_points_returns_expected_center_and_radius() {
        let ((cx, cy), radius) =
            circle_from_3_points((10, 0), (0, 10), (-10, 0)).expect("circle should exist");

        assert_eq!((cx, cy), (0, 0));
        assert!((radius - 10.0).abs() < 0.01);
    }

    #[test]
    fn circle_from_three_collinear_points_returns_none() {
        assert!(circle_from_3_points((0, 0), (10, 0), (20, 0)).is_none());
    }

    #[test]
    fn calibrated_scale_respects_minimum_visible_radius() {
        let min_scale = calibrated_protractor_scale(PROTRACTOR_MIN_CALIBRATION_RADIUS / 2.0);
        let exact_scale = calibrated_protractor_scale(PROTRACTOR_MIN_CALIBRATION_RADIUS);

        assert_eq!(min_scale, 0.4);
        assert_eq!(exact_scale, 0.4);
    }
}
