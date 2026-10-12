#pragma once

// The bones of an animated model of the game (a door, a button, a platform: the extractor writes
// its skeleton and the animations of its art group into its .glb), and their poses: the model's
// vertices moved by its joints (skinning) on the CPU, for the inspector's preview.

#include <string>
#include <vector>

#include "scene/scene.h"

namespace ogle {

struct Skeleton {
  std::vector<int> parent;   // per node of the file (-1: a root)
  std::vector<Mat4> rest;    // per node: its own transform
  std::vector<int> joints;   // per joint of the skin: its node
  std::vector<Mat4> inverse_bind;  // per joint
  struct Channel {
    int node = 0;
    int path = 0;  // 0 translation, 1 rotation, 2 scale
    bool step = false;
    std::vector<float> times;
    std::vector<Vec4> values;
  };
  struct Animation {
    std::string name;
    float duration = 0;  // seconds; 0: a still pose
    std::vector<Channel> channels;
  };
  std::vector<Animation> animations;
};

// Every node's world matrix at `t` seconds into animation `animation` (-1: the rest pose)
std::vector<Mat4> skeleton_pose(const Skeleton& s, int animation, float t);

// Per joint, from the nodes' world matrices: the matrices that move the vertices
std::vector<Mat4> joint_matrices(const Skeleton& s, const std::vector<Mat4>& worlds);

// `out` (made from `bind`: the same primitives and vertices) with its positions and normals moved by
// the joints; each vertex's joints and weights are bind's JOINTS_0 and WEIGHTS_0 (extra_attrs).
// False when they do not match.
bool skin_mesh(const Mesh& bind, const std::vector<Mat4>& joints, Mesh& out);

}  // namespace ogle
