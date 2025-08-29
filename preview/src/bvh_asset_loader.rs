use bevy::{
    animation::{AnimationTargetId, animated_field, gltf_curves::SteppedKeyframeCurve},
    asset::{AssetLoader, AssetPath, LoadContext, io::Reader},
    math::curve::cores::UnevenCoreError,
    platform::collections::HashMap,
    prelude::*,
};
use bvh_anim_parser::{
    parse::load_bvh_from_string,
    types::{BvhData, BvhMetadata, Joint},
};
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
    #[error("Unexpected data format: {0}")]
    UnexpectedData(String),
    #[error("{0}")]
    NotEnoughSamples(UnevenCoreError),
    #[error("Invalid UTF-8 data: {0}")]
    InvalidUTF8(#[from] std::string::FromUtf8Error),
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
        let content = String::from_utf8(bytes)?;
        // .map_err(|e| BvhAssetLoaderError::UnexpectedData(e.to_string()))?;
        let (bvh_meta, bvh_data) = load_bvh_from_string(&content);

        match load_context.asset_path().label() {
            Some(CLIP) => {
                let key_frames = bvh_to_key_frames(&bvh_meta, &bvh_data)?;
                let clip = bvh_to_clip(&bvh_meta, &bvh_data, key_frames)?;
                let clip = load_context.add_labeled_asset(CLIP.to_string(), clip);
                Ok(BvhAsset {
                    clip,
                    ..Default::default()
                })
            }
            Some(SCENE) => {
                let scene = scene_from_bvh(&bvh_meta, &bvh_data)?;

                let scene = load_context.add_labeled_asset(SCENE.to_string(), scene);
                Ok(BvhAsset {
                    scene,
                    ..Default::default()
                })
            }
            Some(KEY_FRAMES) => {
                let key_frames = bvh_to_key_frames(&bvh_meta, &bvh_data)?;

                let key_frames = load_context.add_labeled_asset(KEY_FRAMES.to_string(), key_frames);
                Ok(BvhAsset {
                    key_frames,
                    ..Default::default()
                })
            }
            Some(SKELETON) => {
                let skeleton = JointHierarchy::from_bvh(&bvh_meta, &bvh_data)?;

                let skeleton = load_context.add_labeled_asset(SKELETON.to_string(), skeleton);
                Ok(BvhAsset {
                    skeleton,
                    ..Default::default()
                })
            }
            _ => {
                let key_frames = bvh_to_key_frames(&bvh_meta, &bvh_data)?;
                let skeleton = JointHierarchy::from_bvh(&bvh_meta, &bvh_data)?;
                let clip = bvh_to_clip(&bvh_meta, &bvh_data, key_frames.clone())?;
                let scene = scene_from_bvh(&bvh_meta, &bvh_data)?;

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

fn joint_offset(joint: &Joint, bvh_data: &BvhData) -> Vec3 {
    let offset = bvh_data.rest_local_positions[joint.index];
    Vec3::new(offset[0] as f32, offset[1] as f32, offset[2] as f32)
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
fn scene_from_bvh(
    bvh_meta: &BvhMetadata,
    bvh_data: &BvhData,
) -> Result<Scene, BvhAssetLoaderError> {
    fn spawn_joint(
        bvh_meta: &BvhMetadata,
        bvh_data: &BvhData,
        parent: &mut ChildSpawner,
        joint_index: usize,
    ) {
        let joint = &bvh_meta.joints[joint_index];
        let name = Name::from(joint.name.as_str());

        let mut world = parent.spawn(child_bundle(name.clone(), joint_offset(joint, bvh_data)));
        if let Some(end_site) = joint.endsite.as_ref().map(|e| e.offset) {
            let end_site = Vec3::new(end_site.x as f32, end_site.y as f32, end_site.z as f32);
            if end_site.length() > 0.0 {
                world.with_child(child_bundle(
                    Name::from(format!("{}_end", name.to_string())),
                    end_site,
                ));
            }
        } else {
            world.with_children(|parent| {
                for child in &joint.children {
                    spawn_joint(bvh_meta, bvh_data, parent, *child);
                }
            });
        }
    }

    let mut world = World::default();

    world
        .spawn((
            Transform::default(),
            Visibility::default(),
            AnimationPlayer::default(),
        ))
        .with_children(|spawner| {
            spawn_joint(bvh_meta, bvh_data, spawner, 0);
        });

    Ok(Scene::new(world))
}

//-------------------------------------------------------------------------------------------------
fn bvh_to_key_frames(
    bvh_meta: &BvhMetadata,
    bvh_data: &BvhData,
) -> Result<KeyFrames, BvhAssetLoaderError> {
    let mut joint_translations: HashMap<String, Vec<Vec3>> = HashMap::new();
    let mut joint_rotations: HashMap<String, Vec<Quat>> = HashMap::new();

    for joint in &bvh_meta.joints {
        let joint_positions: Vec<Vec3> = bvh_data.pose_local_positions[joint.index]
            .iter()
            .map(|v| Vec3::new(v.x as f32, v.y as f32, v.z as f32))
            .collect();
        if joint_positions.len() > 0 {
            joint_translations.insert(joint.name.to_string(), joint_positions);
        }

        let rotation_frames: Vec<Quat> = bvh_data.pose_local_rotations[joint.index]
            .iter()
            .map(|q| Quat::from_xyzw(q.v.x as f32, q.v.y as f32, q.v.z as f32, q.s as f32))
            .collect();
        joint_rotations.insert(joint.name.to_string(), rotation_frames);
    }

    Ok(KeyFrames {
        frame_time: bvh_meta.frame_time as f32,
        count: bvh_meta.num_frames,
        joint_translations,
        joint_rotations,
    })
}

//-------------------------------------------------------------------------------------------------
fn bvh_to_clip(
    bvh_meta: &BvhMetadata,
    bvh_data: &BvhData,
    key_frames: KeyFrames,
) -> Result<AnimationClip, BvhAssetLoaderError> {
    let skeleton = JointHierarchy::from_bvh(bvh_meta, bvh_data)?;

    let mut clip = AnimationClip::default();
    let frame_duration = bvh_meta.frame_time as f32;

    for (joint_name, joint_positions) in key_frames.joint_translations {
        let target_id = skeleton.target_id(joint_name.as_str()).ok_or_else(|| {
            BvhAssetLoaderError::UnexpectedData(format!(
                "Could not find target id for joint: {}",
                joint_name
            ))
        })?;

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

        let joint_rotations = create_curve(joint_rotations.into_iter(), frame_duration)?;
        let rotation_property = animated_field!(Transform::rotation);
        let rotation_curve =
            VariableCurve::new(AnimatableCurve::new(rotation_property, joint_rotations));

        clip.add_variable_curve_to_target(target_id, rotation_curve);
    }
    Ok(clip)
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
    pub fn from_bvh(
        bvh_meta: &BvhMetadata,
        bvh_data: &BvhData,
    ) -> Result<Self, BvhAssetLoaderError> {
        fn build_hierarchy(
            bvh_meta: &BvhMetadata,
            bvh_data: &BvhData,
            joint_index: usize,
        ) -> JointHierarchy {
            let joint = &bvh_meta.joints[joint_index];
            JointHierarchy {
                name: joint.name.to_string(),
                offset: joint_offset(joint, bvh_data),
                children: joint
                    .children
                    .iter()
                    .map(|i| build_hierarchy(bvh_meta, bvh_data, *i))
                    .collect(),
                end: joint
                    .endsite
                    .as_ref()
                    .map(|endsite| endsite.offset)
                    .map(|offset| Vec3::new(offset.x as f32, offset.y as f32, offset.z as f32)),
            }
        }

        Ok(build_hierarchy(bvh_meta, bvh_data, 0))
    }

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
