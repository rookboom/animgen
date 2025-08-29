//! Plays an animation on a skinned glTF model of a fox.
mod bvh_asset_loader;
use bevy::{
    ecs::{error, world},
    input::keyboard::Key,
    math::VectorSpace,
    pbr::CascadeShadowConfigBuilder,
    prelude::*,
    scene::SceneInstanceReady,
};
use smooth_bevy_cameras::{
    LookTransformPlugin,
    controllers::unreal::{UnrealCameraBundle, UnrealCameraController, UnrealCameraPlugin},
};
use std::f32::consts::PI;

use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use bvh_asset_loader::BvhAssetLoader;

use crate::bvh_asset_loader::{BvhAsset, BvhAssetLabel, CharacterJoint, JointHierarchy, KeyFrames};

// An example asset that contains a mesh and animation.
const ANIMATION_FILE: &str = "corrected_animations/dataset-1_bow_active_001.bvh";

#[derive(Resource, Default)]
struct AnimationTimeline {
    next_frame_time: f32,
    current_frame: usize,
    anim_index: usize,
}

fn main() {
    App::new()
        .insert_resource(AmbientLight {
            color: Color::WHITE,
            brightness: 2000.,
            ..default()
        })
        .register_type::<CharacterJoint>()
        .add_plugins(DefaultPlugins)
        .add_plugins(LookTransformPlugin)
        .add_plugins(UnrealCameraPlugin::default())
        .add_plugins(EguiPlugin::default())
        .insert_resource(AnimationTimeline::default())
        .insert_resource(LoadState::default())
        .init_asset::<BvhAsset>()
        .init_asset::<KeyFrames>()
        .init_asset::<JointHierarchy>()
        .init_asset_loader::<BvhAssetLoader>()
        // .add_systems(Startup, setup_mesh_and_animation)
        .add_systems(Startup, setup_camera_and_environment)
        .add_systems(Startup, load_animation)
        .add_systems(Update, await_animation_loaded)
        .add_systems(Update, update_animation)
        // .add_systems(Update, draw_characters)
        .add_systems(EguiPrimaryContextPass, timeline_slider_ui)
        .run();
}

// A component that stores a reference to an animation we want to play. This is
// created when we start loading the mesh (see `setup_mesh_and_animation`) and
// read when the mesh has spawned (see `play_animation_once_loaded`).
#[derive(Component)]
struct AnimationToPlay {
    graph_handle: Handle<AnimationGraph>,
    index: AnimationNodeIndex,
}

pub struct Animation {
    key_frames: KeyFrames,
    skeleton: JointHierarchy,
}

#[derive(Default, Resource)]
pub enum LoadState {
    #[default]
    Waiting,
    Loading(Handle<BvhAsset>),
    Loaded(Vec<Animation>),
}

fn load_animation(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load::<BvhAsset>(ANIMATION_FILE);
    commands.insert_resource(LoadState::Loading(handle));
}

fn await_animation_loaded(
    mut load_state: ResMut<LoadState>,
    bvh_assets: Res<Assets<BvhAsset>>,
    key_frames: Res<Assets<KeyFrames>>,
    skeletons: Res<Assets<JointHierarchy>>,
) {
    match *load_state {
        LoadState::Loaded(_) => return,
        LoadState::Waiting => return,
        LoadState::Loading(ref handle) => {
            if let Some(bvh) = bvh_assets.get(handle) {
                match (
                    key_frames.get(&bvh.key_frames),
                    skeletons.get(&bvh.skeleton),
                ) {
                    (Some(kf), Some(skeleton)) => {
                        info!("Loaded animation.");
                        *load_state = LoadState::Loaded(vec![Animation {
                            key_frames: kf.clone(),
                            skeleton: skeleton.clone(),
                        }]);
                    }
                    _ => {
                        error!("Key frames or skeleton not loaded yet.");
                    }
                }
            } else {
                error!("BVH asset not loaded yet.");
            }
        }
    }
}

fn draw_children_rest_position(
    gizmos: &mut Gizmos,
    joint: &JointHierarchy,
    parent_transform: Mat4,
) {
    let joint_transform = parent_transform * Mat4::from_translation(joint.offset);
    let world_position = joint_transform.col(3).xyz();
    let yellow = Color::srgb_u8(255, 255, 0);
    gizmos.sphere(world_position, 2.0, yellow);
    if let Some(end) = joint.end {
        let end_transform = joint_transform * Mat4::from_translation(end);
        gizmos.line(world_position, end_transform.col(3).xyz(), Color::WHITE);
    }
    for child in &joint.children {
        gizmos.line(
            world_position,
            (joint_transform * Mat4::from_translation(child.offset))
                .col(3)
                .xyz(),
            Color::WHITE,
        );
        draw_children_rest_position(gizmos, child, joint_transform);
    }
}

fn draw_rest_position(mut gizmos: Gizmos, load_state: Res<LoadState>) {
    info_once!("Drawing rest position...");
    if let LoadState::Loaded(animations) = &*load_state {
        draw_children_rest_position(&mut gizmos, &animations[0].skeleton, Mat4::IDENTITY);
    }
}

fn draw_pose(
    gizmos: &mut Gizmos,
    skeleton: &JointHierarchy,
    key_frames: &KeyFrames,
    current_frame: usize,
    parent_transform: Mat4,
) {
    let joint_rotation = key_frames.joint_rotations[&skeleton.name][current_frame];
    let joint_transform =
        parent_transform * Mat4::from_rotation_translation(joint_rotation, skeleton.offset);
    let world_position = joint_transform.col(3).xyz();
    let yellow = Color::srgb_u8(255, 255, 0);
    gizmos.sphere(world_position, 2.0, yellow);
    if let Some(end) = skeleton.end {
        let end_transform = joint_transform * Mat4::from_translation(end);
        if end.length() > 0.0 {
            gizmos.line(world_position, end_transform.col(3).xyz(), Color::WHITE);
        }
    }
    for child in &skeleton.children {
        let child_world_position = (joint_transform * Mat4::from_translation(child.offset))
            .col(3)
            .xyz();
        gizmos.line(world_position, child_world_position, Color::WHITE);
        draw_pose(gizmos, child, key_frames, current_frame, joint_transform);
    }
}

fn update_animation(
    mut gizmos: Gizmos,
    mut timeline: ResMut<AnimationTimeline>,
    animation: Res<LoadState>,
    time: Res<Time>,
) {
    // if timeline.next_frame_time >= time.elapsed_secs() {
    //     return;
    // }

    if let LoadState::Loaded(animations) = &*animation {
        let animation = &animations[timeline.anim_index];
        let root_translation = animation.key_frames.joint_translations[&animation.skeleton.name]
            [timeline.current_frame];

        draw_pose(
            &mut gizmos,
            &animation.skeleton,
            &animation.key_frames,
            timeline.current_frame,
            Mat4::from_translation(root_translation),
        );

        // timeline.current_frame += 1;
        // timeline.current_frame %= animation.key_frames.count;
        timeline.next_frame_time = time.elapsed_secs() + animation.key_frames.frame_time;
    }
}

fn setup_mesh_and_animation(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    // Create an animation graph containing a single animation. We want the "run"
    // animation from our example asset, which has an index of two.
    // let (graph, index) = AnimationGraph::from_clip(
    //     asset_server.load(GltfAssetLabel::Animation(0).from_asset(GLTF_PATH)),
    // );
    let (graph, index) = AnimationGraph::from_clip(
        asset_server.load(BvhAssetLabel::Clip.from_asset(ANIMATION_FILE)),
    );
    // miraikomachi_gp -> joint_Root -> Hips

    // Store the animation graph as an asset.
    let graph_handle = graphs.add(graph);

    //Create a component that stores a reference to our animation.
    let animation_to_play = AnimationToPlay {
        graph_handle,
        index,
    };

    // Start loading the asset as a scene and store a reference to it in a
    // SceneRoot component. This component will automatically spawn a scene
    // containing our skeleton hierarchy once it has loaded.
    let skeleton_scene =
        SceneRoot(asset_server.load(BvhAssetLabel::Scene.from_asset(ANIMATION_FILE)));

    // Spawn an entity with our components, and connect it to an observer that
    // will trigger when the scene is loaded and spawned.
    commands
        .spawn((animation_to_play, skeleton_scene))
        .observe(play_animation_when_ready)
        .observe(create_character_visuals_when_ready);
}

fn create_character_visuals_when_ready(
    trigger: Trigger<SceneInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    names: Query<&Name>,
    character_joints: Query<&CharacterJoint>,
) {
    let white = materials.add(Color::WHITE);
    let sphere = meshes.add(Sphere::new(2.0).mesh().ico(3).unwrap());

    for entity in children.iter_descendants(trigger.target()) {
        let name = names.get(entity);
        if let Ok(_) = character_joints.get(entity) {
            info!(
                "Spawning mesh for {}...",
                name.unwrap_or(&Name::from("<unknown>"))
            );
            commands
                .entity(entity)
                .insert((Mesh3d(sphere.clone()), MeshMaterial3d(white.clone())));
        }
    }
}

fn play_animation_when_ready(
    trigger: Trigger<SceneInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    animations_to_play: Query<&AnimationToPlay>,
    mut players: Query<&mut AnimationPlayer>,
) {
    // The entity we spawned in `setup_mesh_and_animation` is the trigger's target.
    // Start by finding the AnimationToPlay component we added to that entity.
    if let Ok(animation_to_play) = animations_to_play.get(trigger.target()) {
        // The SceneRoot component will have spawned the scene as a hierarchy
        // of entities parented to our entity. Since the asset contained a skinned
        // mesh and animations, it will also have spawned an animation player
        // component. Search our entity's descendants to find the animation player.
        for child in children.iter_descendants(trigger.target()) {
            if let Ok(mut player) = players.get_mut(child) {
                // Tell the animation player to start the animation and keep
                // repeating it.
                //
                // If you want to try stopping and switching animations, see the
                // `animated_mesh_control.rs` example.
                player.play(animation_to_play.index).repeat();

                // Add the animation graph. This only needs to be done once to
                // connect the animation player to the mesh.
                commands
                    .entity(child)
                    .insert(AnimationGraphHandle(animation_to_play.graph_handle.clone()));
            }
        }
    }
}

// Spawn a camera and a simple environment with a ground plane and light.
fn setup_camera_and_environment(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let eye = Vec3::new(200.0, 200.0, 200.0);
    let target = Vec3::new(0.0, 100.0, 0.0);
    commands
        .spawn(Camera3d::default())
        .insert(UnrealCameraBundle::new(
            UnrealCameraController {
                enabled: true,
                mouse_translate_sensitivity: Vec2::splat(15.0),
                wheel_translate_sensitivity: 20.0,
                ..default()
            },
            eye,
            target,
            Vec3::Y,
        ));

    // Plane
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(500000.0, 500000.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.3, 0.5, 0.3))),
    ));

    // Light
    commands.spawn((
        Transform::from_rotation(Quat::from_euler(EulerRot::ZYX, 0.0, 1.0, -PI / 4.)),
        DirectionalLight {
            shadows_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            first_cascade_far_bound: 200.0,
            maximum_distance: 400.0,
            ..default()
        }
        .build(),
    ));
}

// fn update_animation(
//     players: Query<&mut AnimationPlayer>,
//     mut event_reader: EventReader<TimelineChanged>,
// ) {
//     for event in event_reader.read() {
//         for mut player in players.iter_mut() {
//             // player.play(event.0);s
//         }
//     }
// }

fn draw_characters(
    mut gizmos: Gizmos,
    characters: Query<(&GlobalTransform, &ChildOf), With<CharacterJoint>>,
) {
    for (transform, parent) in characters.iter() {
        if let Some((parent_transform, _)) = characters.get(parent.0).ok() {
            // Draw a line from the parent joint to the current joint
            gizmos.line(
                parent_transform.translation(),
                transform.translation(),
                Color::WHITE,
            );
        }
    }
}

fn timeline_slider_ui(
    mut contexts: EguiContexts,
    mut timeline: ResMut<AnimationTimeline>,
    mut controllers: Query<&mut UnrealCameraController>,
    animations: Res<LoadState>,
) -> Result {
    if let LoadState::Loaded(animations) = &*animations {
        let animation = &animations[timeline.anim_index];
        let last_frame = animation.key_frames.count - 1;
        let ctx = contexts.ctx_mut()?;
        egui::Window::new("Timeline").show(ctx, |ui| {
            let slider =
                egui::Slider::new(&mut timeline.current_frame, 0..=last_frame).text("Animation");
            ui.add(slider);
        });

        let pointer_over_ui = ctx.is_pointer_over_area();

        for mut controller in controllers.iter_mut() {
            controller.enabled = !pointer_over_ui;
        }
    }
    Ok(())
}
