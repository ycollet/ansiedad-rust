use ansiedad::{canvas::ansi_color, config::Cell};
use eframe::egui::{self, Color32, FontId, Painter, Pos2, Rect, Vec2};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Default)]
pub(super) struct CharacterMesh {
    dirty: bool,
    rect: Option<Rect>,
    font: Option<FontId>,
    pixels_per_point: f32,
    atlas_size: [usize; 2],
    glyphs: HashMap<char, Arc<egui::Galley>>,
    mesh: Arc<egui::Mesh>,
}

impl CharacterMesh {
    pub(super) fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub(super) fn paint(
        &mut self,
        painter: &Painter,
        rect: Rect,
        size: [usize; 2],
        cells: &[Cell],
    ) {
        let cell = Vec2::new(
            rect.width() / size[0] as f32,
            rect.height() / size[1] as f32,
        );
        let font = FontId::monospace(14.0 * cell.y / 16.0);
        let pixels_per_point = painter.ctx().pixels_per_point();
        if self.font.as_ref() != Some(&font) || self.pixels_per_point != pixels_per_point {
            self.glyphs.clear();
            self.dirty = true;
        }
        if self.dirty {
            let used: HashSet<char> = cells
                .iter()
                .map(|cell| cell.character)
                .filter(|c| *c != ' ')
                .collect();
            self.glyphs.retain(|character, _| used.contains(character));
            for character in used {
                self.glyphs.entry(character).or_insert_with(|| {
                    painter.layout_no_wrap(character.to_string(), font.clone(), Color32::WHITE)
                });
            }
        }
        // Refresh layouts even for an unchanged image: egui can reset or grow its font atlas.
        for (&character, galley) in &mut self.glyphs {
            let current =
                painter.layout_no_wrap(character.to_string(), font.clone(), Color32::WHITE);
            self.dirty |= !Arc::ptr_eq(galley, &current);
            *galley = current;
        }
        let atlas_size = painter.ctx().fonts(|fonts| fonts.font_image_size());
        if self.dirty || self.rect != Some(rect) || self.atlas_size != atlas_size {
            let mut glyph_meshes = HashMap::with_capacity(self.glyphs.len());
            for (&character, galley) in &self.glyphs {
                let shapes = vec![egui::epaint::ClippedShape {
                    clip_rect: Rect::EVERYTHING,
                    transform_after_tessellation: egui::emath::TSTransform::IDENTITY,
                    shape: egui::epaint::TextShape::new(
                        Pos2::ZERO,
                        Arc::clone(galley),
                        Color32::WHITE,
                    )
                    .into(),
                }];
                let mut mesh = egui::Mesh::default();
                for primitive in painter.ctx().tessellate(shapes, pixels_per_point) {
                    if let egui::epaint::Primitive::Mesh(part) = primitive.primitive {
                        mesh.append(part);
                    }
                }
                glyph_meshes.insert(character, mesh);
            }
            if Arc::get_mut(&mut self.mesh).is_none() {
                self.mesh = Arc::default();
            }
            let mesh = Arc::get_mut(&mut self.mesh).expect("mesh is uniquely owned");
            mesh.clear();
            mesh.vertices.reserve(cells.len() * 8);
            mesh.indices.reserve(cells.len() * 12);
            let round_text = painter
                .ctx()
                .tessellation_options(|options| options.round_text_to_pixels);
            for (index, value) in cells.iter().enumerate() {
                let min = rect.min
                    + Vec2::new(
                        (index % size[0]) as f32 * cell.x,
                        (index / size[0]) as f32 * cell.y,
                    );
                let cell_rect = Rect::from_min_size(min, cell);
                let [r, g, b] = ansi_color(value.background);
                mesh.add_colored_rect(cell_rect, Color32::from_rgb(r, g, b));
                if let Some(glyph) = glyph_meshes.get(&value.character) {
                    let galley = &self.glyphs[&value.character];
                    let mut position = egui::Align2::CENTER_CENTER
                        .anchor_size(cell_rect.center(), galley.size())
                        .min;
                    if round_text {
                        position.x = (position.x * pixels_per_point).round() / pixels_per_point;
                        position.y = (position.y * pixels_per_point).round() / pixels_per_point;
                    }
                    let offset = mesh.vertices.len() as u32;
                    let [r, g, b] = ansi_color(value.foreground);
                    let color = Color32::from_rgb(r, g, b);
                    mesh.vertices.extend(glyph.vertices.iter().map(|vertex| {
                        egui::epaint::Vertex {
                            pos: vertex.pos + position.to_vec2(),
                            color,
                            ..*vertex
                        }
                    }));
                    mesh.indices
                        .extend(glyph.indices.iter().map(|index| index + offset));
                }
            }
            self.rect = Some(rect);
            self.font = Some(font);
            self.pixels_per_point = pixels_per_point;
            self.atlas_size = atlas_size;
            self.dirty = false;
        }
        painter.add(egui::Shape::Mesh(Arc::clone(&self.mesh)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_reuses_static_geometry_and_refreshes_content_fonts_and_layout() {
        let context = egui::Context::default();
        let mut cache = CharacterMesh::default();
        let mut cells = vec![
            Cell {
                background: 1,
                foreground: 15,
                character: 'E'
            };
            2
        ];
        let mut rect = Rect::from_min_size(Pos2::new(0.25, 0.25), Vec2::new(16.0, 16.0));
        let paint = |cache: &mut CharacterMesh, cells: &[Cell], rect| {
            let mut output = context.run_ui(Default::default(), |ui| {
                cache.paint(ui.painter(), rect, [2, 1], cells);
            });
            output.textures_delta.clear();
        };
        paint(&mut cache, &cells, rect);
        let original = Arc::clone(&cache.mesh);
        paint(&mut cache, &cells, rect);
        assert!(Arc::ptr_eq(&original, &cache.mesh));
        cells[0].character = 'W';
        cache.invalidate();
        paint(&mut cache, &cells, rect);
        assert!(!Arc::ptr_eq(&original, &cache.mesh));
        assert!(cache.glyphs.contains_key(&'W'));
        let changed = Arc::clone(&cache.mesh);
        rect = rect.translate(Vec2::new(5.5, 3.25));
        paint(&mut cache, &cells, rect);
        assert!(!Arc::ptr_eq(&changed, &cache.mesh));
        assert_eq!(cache.mesh.vertices[0].pos, rect.left_top());
        let before_growth = Arc::clone(&cache.mesh);
        let atlas_size = cache.atlas_size;
        let mut output = context.run_ui(Default::default(), |ui| {
            let _ = ui.painter().layout_no_wrap(
                "ABCDEFGHIJKLMNOPQRSTUVWXYZ".into(),
                FontId::monospace(128.0),
                Color32::WHITE,
            );
        });
        output.textures_delta.clear();
        paint(&mut cache, &cells, rect);
        assert_ne!(atlas_size, cache.atlas_size);
        assert!(!Arc::ptr_eq(&before_growth, &cache.mesh));
        let moved = Arc::clone(&cache.mesh);
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap()
            .reverse();
        context.set_fonts(fonts);
        paint(&mut cache, &cells, rect);
        assert!(!Arc::ptr_eq(&moved, &cache.mesh));
        let refonted = Arc::clone(&cache.mesh);
        context.set_pixels_per_point(2.0);
        paint(&mut cache, &cells, rect);
        assert!(!Arc::ptr_eq(&refonted, &cache.mesh));
    }

    #[test]
    fn batched_glyph_matches_standard_centered_text_geometry() {
        let context = egui::Context::default();
        let mut cache = CharacterMesh::default();
        let rect = Rect::from_min_size(Pos2::new(2.25, 3.75), Vec2::new(8.0, 16.0));
        let cell = Cell {
            background: 1,
            foreground: 15,
            character: 'E',
        };
        let mut reference = Vec::new();
        let mut output = context.run_ui(Default::default(), |ui| {
            cache.paint(ui.painter(), rect, [1, 1], &[cell]);
            let galley =
                ui.painter()
                    .layout_no_wrap("E".into(), FontId::monospace(14.0), Color32::WHITE);
            let position = egui::Align2::CENTER_CENTER
                .anchor_size(rect.center(), galley.size())
                .min;
            reference = context.tessellate(
                vec![egui::epaint::ClippedShape::new(
                    Rect::EVERYTHING,
                    egui::epaint::TextShape::new(position, galley, Color32::WHITE),
                )],
                context.pixels_per_point(),
            );
        });
        output.textures_delta.clear();
        let egui::epaint::Primitive::Mesh(reference) = &reference[0].primitive else {
            panic!("expected text mesh")
        };
        assert_eq!(cache.mesh.vertices[4..].len(), reference.vertices.len());
        for (actual, expected) in cache.mesh.vertices[4..].iter().zip(&reference.vertices) {
            assert!((actual.pos - expected.pos).length() < 0.001);
            assert_eq!(actual.uv, expected.uv);
            assert_eq!(actual.color, expected.color);
        }
    }
}
