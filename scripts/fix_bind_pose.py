import sys
import os
import glm
import bvhio
from pathlib import Path

# This converts the Rest Pose so that it is standing upright like at 0,0,0, like most animations.
def remove_motion(joint: bvhio.Joint):
    count = 0
    for (_, transform) in joint.Keyframes:
        transform.PositionWorld = joint.RestPose.PositionWorld
        count += 1

    print(f"Removed motion for {count} keyframes")
    for child in joint.Children:
        remove_motion(child)

def readAsHierarchy(path: str, loadKeyFrames: bool = True) -> bvhio.Joint:
    """Deserialize a .bvh file into a joint hierarchy."""
    bvh = bvhio.readAsBvh(path, loadKeyFrames)
    bvh.Root.Children[0].Offset.x = 0
    bvh.Root.Children[0].Offset.z = 0
    return bvhio.convertBvhToHierarchy(bvh.Root).loadRestPose(recursive=True)


# this script will correct the rest pose of the .bvh files from the bandai namco set
# https://github.com/BandaiNamcoResearchInc/Bandai-Namco-Research-Motiondataset
def modifyFile(source: str, destination: str):
    print(f'Loading {os.path.basename(source)}')
    root = readAsHierarchy(source)
    layout = root.layout()

    # set up T-pose
    print('| Set T-pose')
    root.loadRestPose()
    layout[ 1][0].setEuler((   0,  90,   0))                 # Hips
    layout[ 2][0].setEuler((   0,   0,   0))                # Spine
    layout[ 3][0].setEuler((   0, +90,   0)).roll(-90)      # Chest
    layout[ 4][0].setEuler((   0,   0,   0))                # Neck
    layout[ 5][0].setEuler((   0,   0,   0))                # Head

    layout[ 6][0].setEuler((   0,   0, -90))                # Shoulder_L
    layout[ 7][0].setEuler((   0,   0,   0))                # UpperArm_L
    layout[ 8][0].setEuler((   0,   0,   0))                # LowerArm_L
    layout[ 9][0].setEuler((   0,   0,   0))                # Hand_L

    layout[10][0].setEuler((   0,   0, +90))                # Shoulder_R
    layout[11][0].setEuler((   0,   0,   0))                # UpperArm_R
    layout[12][0].setEuler((   0,   0,   0))                # LowerArm_R
    layout[13][0].setEuler((   0,   0,   0))                # Hand_R

    layout[14][0].setEuler((   0,   0, 180))                # UpperLeg_L
    layout[15][0].setEuler((   0,   0,   0))                # LowerLeg_L
    layout[16][0].setEuler((   0,   0,   0))                # Foot_L
    layout[17][0].setEuler((   0,   0,   0))                # Toes_L

    layout[18][0].setEuler((   0,   0, 180))                # UpperLeg_R
    layout[19][0].setEuler((   0,   0,   0))                # LowerLeg_R
    layout[20][0].setEuler((   0,   0,   0))                # Foot_R
    layout[21][0].setEuler((   0,   0,   0))                # Toes_R
    root.writeRestPose(recursive=True, keep=['position', 'rotation', 'scale'])

    # key frame corrections, turns joints so than Z- axis always points forward
    # DELETE THIS LOOP IF THE BONE ROLL DOES NOT MATTER TO YOU.
    # print('| Correct bone roll')
    for frame in range(*root.getKeyframeRange()):
        root.loadPose(frame, recursive=True)
        layout[ 2][0].roll(-90)                                   # Spine
        layout[ 3][0].roll(-90)                                   # Chest
        layout[ 4][0].roll(-90)                                   # Neck
        layout[ 5][0].roll(-90)                                   # Head
        layout[10][0].roll(180, recursive=True)                   # Shoulder_R
        layout[18][0].roll(180, recursive=True)                   # UpperLeg_R

        layout[ 5][0].Rotation *= glm.angleAxis(glm.radians(-90), (1, 0, 0))  # Head
        layout[ 9][0].Rotation *= glm.angleAxis(glm.radians(-90), (0, 0, 1))  # Hand_L
        layout[13][0].Rotation *= glm.angleAxis(glm.radians(-90), (0, 0, 1))  # Hand_R
        layout[17][0].Rotation *= glm.angleAxis(glm.radians(-90), (0, 0, 1))  # Toes_L
        layout[21][0].Rotation *= glm.angleAxis(glm.radians(-90), (0, 0, 1))  # Toes_R
        root.writePose(frame, recursive=True)

    root = root.Children[0].clearParent()

    bvhio.writeHierarchy(path=destination, root=root, frameTime=1/30)

    # We need to rotate the hips first around the Y-axis, then after it is rotated
    # again around the Y axis. For this reason, we do this in two passes.
    # The first pass sets the character in the T pose and does the first hip rotation.
    # The second pass just rotates the hip around the y axis.
    bvh = bvhio.readAsBvh(destination)
    root = bvhio.convertBvhToHierarchy(bvh.Root).loadRestPose(recursive=True)

    for child in root.Children:
        remove_motion(child)

    layout = root.layout()
    root.loadRestPose()
    layout[0][0].setEuler((   0,  90,   0)) 
    root.writeRestPose(recursive=True, keep=['position', 'rotation', 'scale'])

    print('| Write file')
    bvhio.writeHierarchy(path=destination, root=root, frameTime=1/30)


sourceDir = sys.argv[1]
destinationDir = sys.argv[2]

if not os.path.exists(sourceDir):
    print('Source path does not exist')
    exit()
if not os.path.exists(destinationDir):
    print('Destination path does not exist')
    exit()

files = os.listdir(sourceDir)
files = [item for item in files if item.endswith('.bvh')]
print(f'Found {len(files)} bvh files at given source path:')

for file in files:
    s = os.path.join(sourceDir, file)
    d = os.path.join(destinationDir, file)
    modifyFile(s, d)

print('Done.')


