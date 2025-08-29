use bevy::{
    animation::{AnimationTargetId, animated_field, gltf_curves::SteppedKeyframeCurve},
    asset::{AssetLoader, AssetPath, LoadContext, io::Reader},
    input::keyboard::Key,
    math::curve::cores::UnevenCoreError,
    platform::collections::HashMap,
    prelude::*,
};
use bvh_anim::{Bvh, ChannelType, Joint, errors::LoadError};
use itertools::izip;
use thiserror::Error;

#[derive(TypePath, Asset, Clone)]
pub struct JointHierarchy {
    pub name: String,
    pub offset: Vec3,
    pub end: Option<Vec3>,
    pub children: Vec<JointHierarchy>,
}

#[derive(TypePath, Asset, Default)]
pub struct BvhAsset {
    pub skeleton: Handle<JointHierarchy>,
    pub key_frames: Handle<KeyFrames>,
    pub clip: Handle<AnimationClip>,
    pub scene: Handle<Scene>,
}

#[derive(TypePath, Asset, Clone)]
pub struct KeyFrames {
    pub frame_time: f32,
    pub count: usize,
    pub joint_translations: HashMap<String, Vec<Vec3>>,
    pub joint_rotations: HashMap<String, Vec<Quat>>,
}

pub enum BvhAssetLabel {
    Skeleton,
    KeyFrames,
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

const CLIP: &str = "clip";
const SCENE: &str = "scene";
const SKELETON: &str = "skeleton";
const KEY_FRAMES: &str = "key_frames";

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

        match load_context.asset_path().label() {
            Some(CLIP) => {
                let key_frames = bvh_to_key_frames(&bvh)?;
                let clip = bvh_to_clip(&bvh, key_frames)?;
                let clip = load_context.add_labeled_asset(CLIP.to_string(), clip);
                Ok(BvhAsset {
                    clip,
                    ..Default::default()
                })
            }
            Some(SCENE) => {
                let scene = scene_from_bvh(&bvh)?;

                let scene = load_context.add_labeled_asset(SCENE.to_string(), scene);
                Ok(BvhAsset {
                    scene,
                    ..Default::default()
                })
            }
            Some(KEY_FRAMES) => {
                let key_frames = bvh_to_key_frames(&bvh)?;

                let key_frames = load_context.add_labeled_asset(KEY_FRAMES.to_string(), key_frames);
                Ok(BvhAsset {
                    key_frames,
                    ..Default::default()
                })
            }
            Some(SKELETON) => {
                let skeleton = JointHierarchy::try_from(&bvh)?;

                let skeleton = load_context.add_labeled_asset(SKELETON.to_string(), skeleton);
                Ok(BvhAsset {
                    skeleton,
                    ..Default::default()
                })
            }
            _ => {
                let key_frames = bvh_to_key_frames(&bvh)?;
                let skeleton = JointHierarchy::try_from(&bvh)?;
                let clip = bvh_to_clip(&bvh, key_frames.clone())?;
                let scene = scene_from_bvh(&bvh)?;

                let clip = load_context.add_labeled_asset(CLIP.to_string(), clip);
                let key_frames = load_context.add_labeled_asset(KEY_FRAMES.to_string(), key_frames);
                let scene = load_context.add_labeled_asset(SCENE.to_string(), scene);
                let skeleton = load_context.add_labeled_asset(SKELETON.to_string(), skeleton);
                Ok(BvhAsset {
                    skeleton,
                    key_frames,
                    clip,
                    scene,
                })
            }
        }
    }

    fn extensions(&self) -> &[&str] {
        &["bvh"]
    }
}

fn joint_offset(joint: &Joint) -> Vec3 {
    let offset = joint.offset();
    Vec3::new(offset[0], offset[1], offset[2])
}

fn child_bundle(name: Name, offset: Vec3) -> impl Bundle {
    info!("Creating child proto: {} at {}", name, offset);
    (
        name,
        Transform::from_translation(offset),
        CharacterJoint,
        Visibility::default(),
    )
}

//-------------------------------------------------------------------------------------------------
fn scene_from_bvh(bvh: &Bvh) -> Result<Scene, BvhAssetLoaderError> {
    fn spawn_joint(parent: &mut ChildSpawner, joint: &Joint) {
        let name = Name::from(String::from_utf8(joint.name().to_vec()).unwrap_or_default());

        let mut world = parent.spawn(child_bundle(name.clone(), joint_offset(joint)));
        if let Some(end_site) = joint.end_site() {
            let end_site = Vec3::new(end_site[0], end_site[1], end_site[2]);
            if end_site.length() > 0.0 {
                world.with_child(child_bundle(
                    Name::from(format!("{}_end", name.to_string())),
                    end_site,
                ));
            }
        } else {
            world.with_children(|parent| {
                for child in joint.children() {
                    spawn_joint(parent, &child);
                }
            });
        }
    }

    let mut world = World::default();

    let root_joint = bvh
        .joints()
        .next()
        .ok_or_else(|| BvhAssetLoaderError::UnexpectedData("No root joint found".to_string()))?;

    world
        .spawn((
            Transform::default(),
            Visibility::default(),
            AnimationPlayer::default(),
        ))
        .with_children(|spawner| {
            spawn_joint(spawner, &root_joint);
        });

    Ok(Scene::new(world))
}

//-------------------------------------------------------------------------------------------------
fn bvh_to_key_frames(bvh: &Bvh) -> Result<KeyFrames, BvhAssetLoaderError> {
    // Convert the BVH data to KeyFrames.
    // Note: This conversion may not be correct for all BVH files, but should work for our training data.
    let mut joint_translations: HashMap<String, Vec<Vec3>> = HashMap::new();
    let mut joint_rotations: HashMap<String, Vec<Quat>> = HashMap::new();
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
        if let Some(joint_positions) = extract_joint_positions(&channel_frame_data)? {
            joint_translations.insert(
                String::from_utf8_lossy(joint.name()).to_string(),
                joint_positions.collect(),
            );
        }

        joint_rotations.insert(
            String::from_utf8_lossy(joint.name()).to_string(),
            extract_joint_rotations(&channel_frame_data, rotation_order)?.collect(),
        );
    }

    Ok(KeyFrames {
        frame_time: bvh.frame_time().as_secs_f32(),
        count: bvh.frames().len(),
        joint_translations,
        joint_rotations,
    })
}

//-------------------------------------------------------------------------------------------------
fn bvh_to_clip(bvh: &Bvh, key_frames: KeyFrames) -> Result<AnimationClip, BvhAssetLoaderError> {
    let skeleton = JointHierarchy::try_from(bvh)?;

    let mut clip = AnimationClip::default();

    for (joint_name, joint_positions) in key_frames.joint_translations {
        let target_id = skeleton.target_id(joint_name.as_str()).ok_or_else(|| {
            BvhAssetLoaderError::UnexpectedData(format!(
                "Could not find target id for joint: {}",
                joint_name
            ))
        })?;

        let frame_duration = bvh.frame_time().as_secs_f32();
        let joint_positions = create_curve(joint_positions.into_iter(), frame_duration)?;
        let translation_property = animated_field!(Transform::translation);
        let translation_curve =
            VariableCurve::new(AnimatableCurve::new(translation_property, joint_positions));
        clip.add_variable_curve_to_target(target_id, translation_curve);
    }

    for (joint_name, joint_rotations) in key_frames.joint_rotations {
        let target_id = skeleton.target_id(joint_name.as_str()).ok_or_else(|| {
            BvhAssetLoaderError::UnexpectedData(format!(
                "Could not find target id for joint: {}",
                joint_name
            ))
        })?;

        let frame_duration = bvh.frame_time().as_secs_f32();
        let joint_rotations = create_curve(joint_rotations.into_iter(), frame_duration)?;
        let rotation_property = animated_field!(Transform::rotation);
        let rotation_curve =
            VariableCurve::new(AnimatableCurve::new(rotation_property, joint_rotations));

        clip.add_variable_curve_to_target(target_id, rotation_curve);
    }
    Ok(clip)
}

fn extract_joint_positions(
    channel_frame_data: &HashMap<ChannelType, Vec<f32>>,
) -> Result<Option<impl Iterator<Item = Vec3>>, BvhAssetLoaderError> {
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
    Ok(Some(positions))
}

fn extract_joint_rotations(
    channel_frame_data: &HashMap<ChannelType, Vec<f32>>,
    rotation_order: EulerRot,
) -> Result<impl Iterator<Item = Quat>, BvhAssetLoaderError> {
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
    let rotations = izip!(xs.iter(), ys.iter(), zs.iter()).map(move |(&x, &y, &z)| {
        Quat::from_euler(
            rotation_order,
            x.to_radians(),
            y.to_radians(),
            z.to_radians(),
        )
    });

    Ok(rotations)
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
            BvhAssetLabel::Clip => f.write_str(CLIP),
            BvhAssetLabel::Scene => f.write_str(SCENE),
            BvhAssetLabel::Skeleton => f.write_str(SKELETON),
            BvhAssetLabel::KeyFrames => f.write_str(KEY_FRAMES),
        }
    }
}

//-------------------------------------------------------------------------------------------------
impl TryFrom<&Bvh> for JointHierarchy {
    type Error = BvhAssetLoaderError;

    fn try_from(bvh: &Bvh) -> Result<Self, BvhAssetLoaderError> {
        fn build_hierarchy(joint: Joint) -> JointHierarchy {
            JointHierarchy {
                name: String::from_utf8_lossy(joint.name()).to_string(),
                offset: joint_offset(&joint),
                children: joint.children().map(build_hierarchy).collect(),
                end: joint
                    .end_site()
                    .map(|end| Vec3::new(end[0], end[1], end[2])),
            }
        }
        let root_joint = bvh
            .joints()
            .next()
            .ok_or(BvhAssetLoaderError::UnexpectedData(
                "No root joint found".to_string(),
            ))?;
        Ok(build_hierarchy(root_joint))
    }
}

//-------------------------------------------------------------------------------------------------
impl TryFrom<&Bvh> for KeyFrames {
    type Error = BvhAssetLoaderError;

    fn try_from(bvh: &Bvh) -> Result<Self, BvhAssetLoaderError> {
        let key_frames = bvh_to_key_frames(bvh)?;
        Ok(key_frames)
    }
}

//-------------------------------------------------------------------------------------------------
fn target_id<'a>(
    joint_hierarchy: &'a JointHierarchy,
    bone_name: &str,
    path: &mut Vec<&'a str>,
) -> Option<AnimationTargetId> {
    path.push(&joint_hierarchy.name);

    // This is the last node. Make sure the bone name matches.
    if bone_name == joint_hierarchy.name {
        return Some(AnimationTargetId::from_iter(path.iter()));
    }

    for child in joint_hierarchy.children.iter() {
        if let Some(id) = target_id(child, bone_name, path) {
            return Some(id);
        } else {
            path.pop();
        }
    }
    None
}
impl JointHierarchy {
    pub fn target_id(&self, bone_name: &str) -> Option<AnimationTargetId> {
        let mut path = vec![];
        target_id(&self, bone_name, &mut path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::animation::AnimationTargetId;

    #[test]
    fn test_target_id_simple_hierarchy() {
        // Build a simple hierarchy: root -> child1 -> child2
        let child2 = JointHierarchy {
            name: "child2".to_string(),
            offset: Vec3::ZERO,
            children: vec![],
            end: None,
        };
        let child1 = JointHierarchy {
            name: "child1".to_string(),
            offset: Vec3::ZERO,
            children: vec![child2],
            end: None,
        };
        let root = JointHierarchy {
            name: "root".to_string(),
            offset: Vec3::ZERO,
            children: vec![child1],
            end: None,
        };

        // Test finding child2
        let id = root.target_id("child2");
        assert!(id.is_some());
        let id = id.unwrap();
        let expected = AnimationTargetId::from_iter(["root", "child1", "child2"]);
        assert_eq!(id, expected);

        // Test finding root
        let id = root.target_id("root");
        assert!(id.is_some());
        let id = id.unwrap();
        let expected = AnimationTargetId::from_iter(["root"]);
        assert_eq!(id, expected);

        // Test not found
        let id = root.target_id("not_a_joint");
        assert!(id.is_none());
    }
}
