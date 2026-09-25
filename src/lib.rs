use std::{collections::HashMap, thread::JoinHandle};

use bevy_app::{Plugin, PostUpdate};
use bevy_asset::{
    AssetEvent, AssetHandleProvider, AssetId, AssetPath, AssetServer, Assets, Handle,
    RenderAssetUsages,
};
use bevy_ecs::{
    event::EventReader,
    resource::Resource,
    schedule::{IntoScheduleConfigs, common_conditions::on_event},
    system::{Res, ResMut},
    world::{FromWorld, World},
};
use bevy_image::{Image, TextureFormatPixelInfo};
use wgpu_types::{Extent3d, TextureViewDescriptor, TextureViewDimension};

use crate::convert::CubeSide;

pub mod convert;

pub struct EquirectangularPlugin;
impl Plugin for EquirectangularPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        app.init_resource::<EquirectManager>();
        app.add_systems(
            PostUpdate,
            (
                start_conversions.run_if(on_event::<AssetEvent<Image>>),
                finish_conversions
                    .run_if(|manager: Res<EquirectManager>| !manager.converting.is_empty()),
            ),
        );
    }
}

// converting takes seconds for big skies, so it runs off the main thread
fn start_conversions(
    mut images: ResMut<Assets<Image>>,
    mut manager: ResMut<EquirectManager>,
    mut reader: EventReader<AssetEvent<Image>>,
) {
    for event in reader.read() {
        if let AssetEvent::Added { id } = event
            && let Some(cubemap) = manager.handles.get(id)
            && let Some(src) = images.get_mut(*id)
            && let Some(data) = src.data.take()
        {
            // only the conversion needs the source, so keep it off the gpu entirely
            src.asset_usage = RenderAssetUsages::MAIN_WORLD;
            let (width, height) = (src.width(), src.height());
            let format = src.texture_descriptor.format;
            let res = cubemap.res;
            let dst = cubemap.dst.clone();
            let task =
                std::thread::spawn(move || cubemap_from_raw(width, height, &data, format, res));
            manager.converting.push((dst, task));
        }
        if let AssetEvent::Unused { id } | AssetEvent::Removed { id } = event {
            manager.handles.remove(id);
        }
    }
}

fn finish_conversions(mut images: ResMut<Assets<Image>>, mut manager: ResMut<EquirectManager>) {
    let converting = std::mem::take(&mut manager.converting);
    for (dst, task) in converting {
        if !task.is_finished() {
            manager.converting.push((dst, task));
            continue;
        }
        if let Ok(image) = task.join() {
            images.insert(&dst, image);
        }
    }
}

#[derive(Resource)]
pub struct EquirectManager {
    asset_server: AssetServer,
    image_handle_provider: AssetHandleProvider,
    handles: HashMap<AssetId<Image>, EquirectCubemap>,
    converting: Vec<(Handle<Image>, JoinHandle<Image>)>,
}
struct EquirectCubemap {
    // keeps the source loaded, so asking for the same path again finds this entry
    _src: Handle<Image>,
    dst: Handle<Image>,
    res: u32,
}
impl EquirectManager {
    pub fn load_equirect_as_cubemap<'a>(
        &mut self,
        path: impl Into<AssetPath<'a>>,
        res: u32,
    ) -> Handle<Image> {
        let src = self.asset_server.load(path);

        self.handles
            .entry(src.id())
            .or_insert_with(|| EquirectCubemap {
                _src: src,
                dst: self.image_handle_provider.reserve_handle().typed::<Image>(),
                res,
            })
            .dst
            .clone()
    }
}
impl FromWorld for EquirectManager {
    fn from_world(world: &mut World) -> Self {
        let asset_server = world.resource::<AssetServer>().clone();
        let image_handle_provider = world.resource::<Assets<Image>>().get_handle_provider();
        Self {
            asset_server,
            image_handle_provider,
            handles: HashMap::new(),
            converting: Vec::new(),
        }
    }
}

pub fn cubemap_from_equirectangular(equirect: &Image, cubemap_res: u32) -> Image {
    cubemap_from_raw(
        equirect.width(),
        equirect.height(),
        equirect.data.as_ref().unwrap(),
        equirect.texture_descriptor.format,
        cubemap_res,
    )
}

fn cubemap_from_raw(
    width: u32,
    height: u32,
    data: &[u8],
    format: wgpu_types::TextureFormat,
    cubemap_res: u32,
) -> Image {
    let face_size = cubemap_res * cubemap_res * format.pixel_size() as u32;
    let out_size = face_size * 6;
    let mut out = vec![0u8; out_size as usize];
    // every face is independent, so they all get a core
    let faces = std::thread::scope(|scope| {
        CubeSide::ALL
            .map(|face| {
                scope.spawn(move || {
                    (
                        face,
                        face.gen_face(width, height, data, cubemap_res, format),
                    )
                })
            })
            .map(|task| task.join().unwrap())
    });
    for (face, face_data) in faces {
        let index = (face_size * face.get_cubemap_index()) as usize;
        let index_end = index + face_size as usize;
        out[index..index_end].copy_from_slice(&face_data);
    }

    let mut image = Image::new(
        Extent3d {
            width: cubemap_res,
            height: cubemap_res,
            depth_or_array_layers: 6,
        },
        wgpu_types::TextureDimension::D2,
        out,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..Default::default()
    });
    image
}
