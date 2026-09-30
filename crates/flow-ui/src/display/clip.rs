//! Convex polygon clipping used for transformed rounded Frame masks.
use gpui_flow_scene::Point;
fn cross(a: Point, b: Point) -> f32 {
    a.x * b.y - a.y * b.x
}
/// Clip a polygon against a convex mask. Both winding directions are supported.
pub fn clip_polygon(mut subject: Vec<Point>, clip: &[Point]) -> Vec<Point> {
    if clip.len() < 3 {
        return vec![];
    }
    let area: f32 = clip
        .iter()
        .zip(clip.iter().cycle().skip(1))
        .take(clip.len())
        .map(|(a, b)| cross(*a, *b))
        .sum();
    let sign = if area >= 0. { 1. } else { -1. };
    for (&a, &b) in clip
        .iter()
        .zip(clip.iter().cycle().skip(1))
        .take(clip.len())
    {
        if subject.is_empty() {
            break;
        }
        let input = std::mem::take(&mut subject);
        let mut previous = *input.last().unwrap();
        let edge = b - a;
        let mut previous_distance = sign * cross(edge, previous - a);
        for current in input {
            let distance = sign * cross(edge, current - a);
            if (distance >= 0.) != (previous_distance >= 0.) {
                let denominator = previous_distance - distance;
                if denominator.abs() > f32::EPSILON {
                    subject
                        .push(previous + (current - previous) * (previous_distance / denominator));
                }
            }
            if distance >= 0. {
                subject.push(current);
            }
            previous = current;
            previous_distance = distance;
        }
    }
    subject
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clips_both_windings() {
        let p = vec![
            Point::new(-1., -1.),
            Point::new(3., -1.),
            Point::new(3., 3.),
            Point::new(-1., 3.),
        ];
        let mask = vec![
            Point::new(0., 0.),
            Point::new(2., 0.),
            Point::new(2., 2.),
            Point::new(0., 2.),
        ];
        for mask in [mask.clone(), mask.into_iter().rev().collect()] {
            let result = clip_polygon(p.clone(), &mask);
            assert_eq!(result.len(), 4);
            assert!(
                result
                    .iter()
                    .all(|p| p.x >= 0. && p.x <= 2. && p.y >= 0. && p.y <= 2.)
            );
        }
    }
}
