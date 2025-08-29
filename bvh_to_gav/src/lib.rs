use anyhow::Result;
use bevy_math::{Quat, Vec3};
use bvh_anim_parser::types::BvhData;
use ndarray::{Array3, ShapeError};

pub struct Animation {
    pub root_positions: Vec<Vec3>,
    pub joint_rotations: Vec<Vec<Quat>>,
}

impl Animation {
    pub fn joint_count(&self) -> usize {
        self.joint_rotations.len()
    }

    pub fn frame_count(&self) -> usize {
        self.root_positions.len()
    }
}
/// BVH to GAV (Geometric Algebra Animation Vector)
pub fn bvh_to_gav(bvh_data: &BvhData, frame_count: usize) -> Result<Array3<f32>, ShapeError> {
    let joint_count = bvh_data.pose_local_rotations.len();
    let mut data = Vec::with_capacity(frame_count * (joint_count + 1));
    for frame in &bvh_data.pose_local_positions[0] {
        data.push(frame.x as f32);
        data.push(frame.y as f32);
        data.push(frame.z as f32);
    }

    for joint in &bvh_data.pose_local_rotations {
        for quat in joint {
            // Note that we only store the vector part of the quaternion
            // This is the equivalent of the bivector. When converting back to a quaternion,
            // The magnitude of the rotation can easily be recomputed since the original quaternion
            // was normalized and the bivector is not.
            data.push(quat.v.x as f32);
            data.push(quat.v.y as f32);
            data.push(quat.v.z as f32);
        }
    }

    Array3::from_shape_vec((joint_count + 1, frame_count, 3), data)
}

/// BVH to GAV (Geometric Algebra Animation Vector)
pub fn gav_to_animation(gav_data: Array3<f32>) -> Result<Animation> {
    let (curve_count, frame_count, _) = gav_data.dim();
    let mut root_positions = Vec::with_capacity(frame_count);
    let mut joint_rotations = vec![Vec::with_capacity(frame_count); curve_count - 1];

    for (curve_index, chunk) in gav_data.axis_iter(ndarray::Axis(0)).enumerate() {
        for frame_value in chunk.axis_iter(ndarray::Axis(0)) {
            assert_eq!(frame_value.len(), 3);
            if curve_index == 0 {
                root_positions.push(Vec3::new(frame_value[0], frame_value[1], frame_value[2]));
            } else {
                let joint_index = curve_index - 1;
                joint_rotations[joint_index].push(
                    Quat::from_xyzw(frame_value[0], frame_value[1], frame_value[2], 0.0)
                        .normalize(),
                );
            }
        }
    }
    Ok(Animation {
        root_positions,
        joint_rotations,
    })
}
