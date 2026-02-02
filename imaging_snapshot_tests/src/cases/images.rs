// Copyright 2026 the Imaging Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use imaging::{Composite, Draw, Geometry, Sink, StrokeStyle};
use kurbo::{Affine, BezPath, Point, Rect, RoundedRect};
use peniko::{BlendMode, Brush, Color, Extend, Fill, ImageBrush, ImageQuality, Mix};

use super::SnapshotCase;
use super::util::{background, test_image};

pub(crate) struct GmImageBrushes;
impl SnapshotCase for GmImageBrushes {
    fn name(&self) -> &'static str {
        "gm_image_brushes"
    }

    fn supports_backend(&self, backend: &str) -> bool {
        matches!(backend, "skia" | "vello_cpu" | "vello")
    }

    fn run(&self, sink: &mut dyn Sink, width: f64, height: f64) {
        background(sink, width, height, Color::from_rgb8(244, 244, 240));

        sink.draw(Draw::Fill {
            transform: Affine::IDENTITY,
            fill_rule: Fill::NonZero,
            paint: Brush::Image(
                ImageBrush::new(test_image())
                    .with_extend(Extend::Pad)
                    .with_quality(ImageQuality::Medium),
            ),
            paint_transform: Some(Affine::translate((-18.0, -12.0))),
            shape: Geometry::RoundedRect(RoundedRect::new(
                width * 0.08,
                height * 0.12,
                width * 0.42,
                height * 0.84,
                24.0,
            )),
            composite: Composite::default(),
        });

        sink.draw(Draw::Fill {
            transform: Affine::IDENTITY,
            fill_rule: Fill::NonZero,
            paint: Brush::Image(
                ImageBrush::new(test_image())
                    .with_x_extend(Extend::Reflect)
                    .with_y_extend(Extend::Pad)
                    .with_quality(ImageQuality::Medium),
            ),
            paint_transform: Some(Affine::translate((-104.0, -8.0))),
            shape: Geometry::Path(diamond(
                width * 0.72,
                height * 0.48,
                width * 0.2,
                height * 0.28,
            )),
            composite: Composite::new(BlendMode::from(Mix::Multiply), 1.0),
        });

        sink.draw(Draw::Stroke {
            transform: Affine::IDENTITY,
            stroke: StrokeStyle::new(20.0),
            paint: Brush::Image(
                ImageBrush::new(test_image())
                    .with_extend(Extend::Reflect)
                    .with_quality(ImageQuality::Low),
            ),
            paint_transform: Some(Affine::translate((-126.0, -120.0))),
            shape: Geometry::RoundedRect(RoundedRect::new(
                width * 0.5,
                height * 0.58,
                width * 0.9,
                height * 0.9,
                30.0,
            )),
            composite: Composite::default(),
        });

        sink.draw(Draw::Fill {
            transform: Affine::IDENTITY,
            fill_rule: Fill::NonZero,
            paint: Brush::Solid(Color::from_rgba8(255, 255, 255, 170)),
            paint_transform: None,
            shape: Geometry::Rect(Rect::new(
                width * 0.46,
                height * 0.54,
                width * 0.94,
                height * 0.94,
            )),
            composite: Composite::default(),
        });
    }
}

fn diamond(cx: f64, cy: f64, rx: f64, ry: f64) -> BezPath {
    let mut path = BezPath::new();
    path.move_to(Point::new(cx, cy - ry));
    path.line_to(Point::new(cx + rx, cy));
    path.line_to(Point::new(cx, cy + ry));
    path.line_to(Point::new(cx - rx, cy));
    path.close_path();
    path
}
