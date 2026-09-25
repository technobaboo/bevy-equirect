use core::{
    f32::{self, consts::PI},
    iter::Iterator,
};

use bevy_image::TextureFormatPixelInfo;
use glam::{Vec2, Vec3, vec3};
use wgpu_types::TextureFormat;

#[derive(Copy, Clone, Hash, Debug, Eq, PartialEq)]
pub enum CubeSide {
    X,
    NegX,
    Y,
    NegY,
    Z,
    NegZ,
}

impl CubeSide {
    pub const ALL: [CubeSide; 6] = [
        CubeSide::X,
        CubeSide::NegX,
        CubeSide::Y,
        CubeSide::NegY,
        CubeSide::Z,
        CubeSide::NegZ,
    ];
    pub fn gen_face(
        &self,
        equirect_width: u32,
        equirect_height: u32,
        equirect_data: &[u8],
        res: u32,
        format: TextureFormat,
    ) -> Vec<u8> {
        let bpp = format.pixel_size() as u32;
        let out_size = res * res * bpp;
        let mut out = vec![0u8; out_size as usize];
        // rows outside, so the writes below walk through `out` in order
        for (x, y) in (0..res).flat_map(|y| (0..res).map(move |x| (x, y))) {
            let pos = self.get_xyz_form_pixel_coords(x, y, res);
            let angles = self.get_angles_from_xyz(pos);
            let uv = self.get_uv_from_angles(angles);
            let pixel_x = (uv.x * (equirect_width as f32)).floor() as u32;
            let pixel_y = (uv.y * (equirect_height as f32)).floor() as u32;
            let index = ((pixel_y * equirect_width * bpp) + (pixel_x * bpp)) as usize;
            let out_index = ((y * res * bpp) + (x * bpp)) as usize;
            let s = &mut out[out_index..(out_index + bpp as usize)];
            s.copy_from_slice(&equirect_data[index..(index + bpp as usize)]);

            // TODO: MSAA
        }
        out
    }
    pub const fn get_cubemap_index(&self) -> u32 {
        match self {
            CubeSide::X => 0,
            CubeSide::NegX => 1,
            CubeSide::Y => 2,
            CubeSide::NegY => 3,
            CubeSide::Z => 4,
            CubeSide::NegZ => 5,
        }
    }
    pub fn get_angles_from_xyz(&self, pos: Vec3) -> Vec2 {
        let pos = pos.normalize();
        let theta = pos.x.atan2(-pos.z);
        let phi = (pos.y).asin();
        Vec2::new(phi, theta)
    }
    pub fn get_uv_from_angles(&self, angles: Vec2) -> Vec2 {
        Vec2::new(angles.y / (2.0 * PI) + 0.5, 1.0 - (angles.x / PI + 0.5))
    }
    pub fn get_xyz_form_pixel_coords(&self, pixel_x: u32, pixel_y: u32, res: u32) -> Vec3 {
        let offset_x = ((pixel_x as f32 / (res - 1) as f32) * 2.0) - 1.0;
        let offset_y = ((pixel_y as f32 / (res - 1) as f32) * 2.0) - 1.0;
        match self {
            CubeSide::X => vec3(1.0, -offset_y, -offset_x),
            CubeSide::NegX => vec3(-1.0, -offset_y, offset_x),
            CubeSide::Y => vec3(offset_x, 1.0, offset_y),
            CubeSide::NegY => vec3(offset_x, -1.0, -offset_y),
            CubeSide::Z => vec3(offset_x, -offset_y, 1.0),
            CubeSide::NegZ => vec3(-offset_x, -offset_y, -1.0),
        }
    }
}

#[test]
fn row_order_matches_the_original_column_order() {
    let (width, height, res) = (64, 32, 16);
    let format = TextureFormat::Rgba8Unorm;
    let mut seed = 0x2545_f491_u32;
    let data: Vec<u8> = (0..width * height * 4)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        })
        .collect();
    for face in CubeSide::ALL {
        let bpp = 4;
        let mut expected = vec![0u8; (res * res * bpp) as usize];
        for (x, y) in (0..res).flat_map(|x| (0..res).map(move |y| (x, y))) {
            let uv = face.get_uv_from_angles(
                face.get_angles_from_xyz(face.get_xyz_form_pixel_coords(x, y, res)),
            );
            let pixel_x = (uv.x * width as f32).floor() as u32;
            let pixel_y = (uv.y * height as f32).floor() as u32;
            let index = ((pixel_y * width * bpp) + (pixel_x * bpp)) as usize;
            let out_index = ((y * res * bpp) + (x * bpp)) as usize;
            expected[out_index..out_index + bpp as usize]
                .copy_from_slice(&data[index..index + bpp as usize]);
        }
        assert_eq!(
            face.gen_face(width, height, &data, res, format),
            expected,
            "{face:?}"
        );
    }
}
