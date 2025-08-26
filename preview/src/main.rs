//! Plays an animation on a skinned glTF model of a fox.
mod bvh_asset_loader;
use bevy::{pbr::CascadeShadowConfigBuilder, prelude::*, scene::SceneInstanceReady};
use smooth_bevy_cameras::{
    LookTransform, LookTransformPlugin,
    controllers::unreal::{UnrealCameraBundle, UnrealCameraController, UnrealCameraPlugin},
};
use std::f32::consts::PI;

use bvh_asset_loader::BvhAssetLoader;

use crate::bvh_asset_loader::{BvhAsset, BvhAssetLabel, CharacterJoint};

// An example asset that contains a mesh and animation.
const ANIMATION_FILE: &str = "corrected_animations/dataset-1_bow_active_001.bvh";

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
        .init_asset::<BvhAsset>()
        .init_asset_loader::<BvhAssetLoader>()
        .add_systems(Startup, setup_mesh_and_animation)
        .add_systems(Startup, setup_camera_and_environment)
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
            UnrealCameraController::default(),
            eye,
            target,
            Vec3::Y,
        ));

    // Camera
    // commands.spawn((
    //     Camera3d::default(),
    //     Transform::from_xyz(5.0, 5.0, 5.0).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y),
    // ));

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
