use bevy::{
    animation::{AnimationTargetId, animated_field, gltf_curves::SteppedKeyframeCurve},
    asset::{AssetLoader, AssetPath, LoadContext, io::Reader},
    math::curve::cores::UnevenCoreError,
    platform::collections::HashMap,
    prelude::*,
};
use bvh_anim::{Bvh, ChannelType, Joint, errors::LoadError};
use itertools::izip;
use thiserror::Error;

#[derive(TypePath, Asset)]
pub struct BvhAsset {
    pub clip: Handle<AnimationClip>,
    // The scene is a visual representation of the character skeleton.
    // Useful for visualizing the character's pose and animation.
    pub scene: Handle<Scene>,
}

pub enum BvhAssetLabel {
    Clip,
    Scene,
}

#[derive(Component, Reflect, Clone, PartialEq, Eq)]
#[reflect(Component)]
pub struct CharacterJoint;

#[derive(Default)]
pub struct BvhAssetLoader;

/// Possible errors that can be produced by [`BvhAssetLoader`]
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum BvhAssetLoaderError {
    #[error("Could not load asset: {0}")]
    Io(#[from] std::io::Error),
    #[error("Could not parse BVH: {0}")]
    LoadError(#[from] LoadError),
    #[error("Unexpected data format: {0}")]
    UnexpectedData(String),
    #[error("{0}")]
    NotEnoughSamples(UnevenCoreError),
}

impl AssetLoader for BvhAssetLoader {
    type Asset = BvhAsset;
    type Settings = ();
    type Error = BvhAssetLoaderError;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let bvh = bvh_anim::from_bytes(&bytes)?;
        let clip = bvh_to_clip(&bvh)?;
        let scene = scene_from_bvh(&bvh)?;
        let clip = load_context.add_labeled_asset("clip".to_string(), clip);
        let scene = load_context.add_labeled_asset("scene".to_string(), scene);
        Ok(BvhAsset { clip, scene })
    }

    fn extensions(&self) -> &[&str] {
        &["bvh"]
    }
}

fn joint_transform(joint: &Joint) -> Transform {
    let offset = joint.offset();
    Transform::from_translation(Vec3::new(offset[0], offset[1], offset[2]))
}

//-------------------------------------------------------------------------------------------------
fn scene_from_bvh(bvh: &Bvh) -> Result<Scene, BvhAssetLoaderError> {
    fn spawn_joint<'a, 'w>(
        world: &'a mut EntityWorldMut<'w>,
        joint: &Joint,
    ) -> &'a mut EntityWorldMut<'w> {
        let name = Name::from(String::from_utf8(joint.name().to_vec()).unwrap_or_default());
        let world = world.with_child((name.clone(), joint_transform(joint), CharacterJoint));
        if let Some(end_site) = joint.end_site() {
            let end_site = Vec3::new(end_site[0], end_site[1], end_site[2]);
            if end_site.length() > 0.0 {
                world.with_child((
                    Name::from(format!("{}_end", name.to_string())),
                    Transform::from_translation(end_site),
                    CharacterJoint,
                ));
            }
        } else {
            for child in joint.children() {
                spawn_joint(world, &child);
            }
        }

        world
    }

    let mut world = World::default();

    let root_joint = bvh
        .joints()
        .next()
        .ok_or_else(|| BvhAssetLoaderError::UnexpectedData("No root joint found".to_string()))?;

    let mut entity_world = world.spawn_empty();

    spawn_joint(&mut entity_world, &root_joint);

    Ok(Scene::new(world))
}
//-------------------------------------------------------------------------------------------------
fn bvh_to_clip(bvh: &Bvh) -> Result<AnimationClip, BvhAssetLoaderError> {
    // Convert the BVH data to an AnimationClip.
    // Note: This conversion may not be correct for all BVH files, but should work for our training data.
    let mut clip = AnimationClip::default();
    let mut channel_frame_data: HashMap<ChannelType, Vec<f32>> = HashMap::new();

    let mut rotation_order = [
        ChannelType::RotationX,
        ChannelType::RotationY,
        ChannelType::RotationZ,
    ];
    let mut rotation_index;
    for joint in bvh.joints() {
        rotation_index = 0;
        for channel in joint.channels().iter() {
            let channel_type = channel.channel_type();
            let frame_data = channel_frame_data.entry(channel_type).or_default();
            frame_data.clear();
            for frame in bvh.frames() {
                let value = frame.get(channel).expect("Frame data missing");
                frame_data.push(*value);
            }
            match channel_type {
                ChannelType::RotationX | ChannelType::RotationY | ChannelType::RotationZ => {
                    rotation_order[rotation_index] = channel_type;
                    rotation_index += 1;
                }
                _ => {}
            }
        }

        let rotation_order = match rotation_order {
            [
                ChannelType::RotationX,
                ChannelType::RotationY,
                ChannelType::RotationZ,
            ] => EulerRot::XYZ,
            [
                ChannelType::RotationZ,
                ChannelType::RotationX,
                ChannelType::RotationY,
            ] => EulerRot::ZXY,
            _ => {
                return Err(BvhAssetLoaderError::UnexpectedData(format!(
                    "Unexpected euler rotation order. Expected XYZ but got: {:?}",
                    rotation_order
                )));
            }
        };
        info!("Adding joint {} ", String::from_utf8_lossy(joint.name()));
        let target_id = get_joint_target_id(&joint);

        let frame_duration = bvh.frame_time().as_secs_f32();
        if let Some(joint_positions) = extract_joint_positions(&channel_frame_data, frame_duration)?
        {
            let translation_property = animated_field!(Transform::translation);
            let translation_curve =
                VariableCurve::new(AnimatableCurve::new(translation_property, joint_positions));
            clip.add_variable_curve_to_target(target_id, translation_curve);
        }

        let joint_rotations =
            extract_joint_rotations(&channel_frame_data, rotation_order, frame_duration)?;
        let rotation_property = animated_field!(Transform::rotation);
        let rotation_curve =
            VariableCurve::new(AnimatableCurve::new(rotation_property, joint_rotations));

        clip.add_variable_curve_to_target(target_id, rotation_curve);
    }
    Ok(clip)
}

fn add_joint_name(joint: &Joint, path: &mut Vec<Name>) {
    path.push(Name::from(
        String::from_utf8_lossy(joint.name()).to_string(),
    ));
    if let Some(parent) = joint.parent() {
        add_joint_name(&parent, path);
    }
}
fn get_joint_target_id(joint: &Joint) -> AnimationTargetId {
    let mut names = Vec::new();
    add_joint_name(joint, &mut names);
    AnimationTargetId::from_names(names.iter().rev())
}

fn extract_joint_positions(
    channel_frame_data: &HashMap<ChannelType, Vec<f32>>,
    frame_duration: f32,
) -> Result<Option<SteppedKeyframeCurve<Vec3>>, BvhAssetLoaderError> {
    let xs = channel_frame_data
        .get(&ChannelType::PositionX)
        .expect("Missing PositionX channel");
    let ys = channel_frame_data
        .get(&ChannelType::PositionY)
        .expect("Missing PositionY channel");
    let zs = channel_frame_data
        .get(&ChannelType::PositionZ)
        .expect("Missing PositionZ channel");

    if xs.len() != ys.len() || xs.len() != zs.len() {
        return Err(BvhAssetLoaderError::UnexpectedData(
            "Position channels have different lengths".to_string(),
        ));
    }

    if xs.is_empty() {
        return Ok(None);
    }

    let positions = izip!(xs.iter(), ys.iter(), zs.iter()).map(|(&x, &y, &z)| Vec3::new(x, y, z));
    create_curve(positions, frame_duration).map(|c| Some(c))
}

fn extract_joint_rotations(
    channel_frame_data: &HashMap<ChannelType, Vec<f32>>,
    rotation_order: EulerRot,
    frame_duration: f32,
) -> Result<SteppedKeyframeCurve<Quat>, BvhAssetLoaderError> {
    let xs = channel_frame_data
        .get(&ChannelType::RotationX)
        .expect("Missing RotationX channel");
    let ys = channel_frame_data
        .get(&ChannelType::RotationY)
        .expect("Missing RotationY channel");
    let zs = channel_frame_data
        .get(&ChannelType::RotationZ)
        .expect("Missing RotationZ channel");

    if xs.len() != ys.len() || xs.len() != zs.len() {
        return Err(BvhAssetLoaderError::UnexpectedData(
            "Position channels have different lengths".to_string(),
        ));
    }

    if xs.is_empty() {
        return Err(BvhAssetLoaderError::UnexpectedData(
            "Missing Rotation channels".to_string(),
        ));
    }
    let rotations = izip!(xs.iter(), ys.iter(), zs.iter())
        .map(|(&x, &y, &z)| Quat::from_euler(rotation_order, x, y, z));
    create_curve(rotations, frame_duration)
}

fn create_curve<T, I: Iterator<Item = T>>(
    values: I,
    frame_duration: f32,
) -> Result<SteppedKeyframeCurve<T>, BvhAssetLoaderError> {
    let keyframes = values.into_iter().enumerate().map(|(i, pos)| {
        let time = i as f32 * frame_duration;
        (time, pos)
    });

    SteppedKeyframeCurve::new(keyframes).map_err(|e| BvhAssetLoaderError::NotEnoughSamples(e))
}

impl BvhAssetLabel {
    pub fn from_asset(&self, path: impl Into<AssetPath<'static>>) -> AssetPath<'static> {
        path.into().with_label(self.to_string())
    }
}
impl core::fmt::Display for BvhAssetLabel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BvhAssetLabel::Clip => f.write_str("clip"),
            BvhAssetLabel::Scene => f.write_str("scene"),
        }
    }
}
