// Player physics: swept AABB against voxels, port of mc-rs world/physics.rs (dry land + simple swimming).
#pragma once
#include <functional>
#include "vec.hpp"
#include "world.hpp"

namespace mc {

constexpr float HALF_W = 0.3f, HALF_H = 0.9f;   // 0.6 x 1.8 x 0.6 box
constexpr float EYE_OFFSET = 0.72f;             // eye is 1.62 above the feet; pos is the box centre
constexpr float WALK_SPEED = 4.3f;              // m/s with the stick fully pressed

struct Player {
    Vec3 pos, vel;  // pos = centre of the box
    bool on_ground = false;
    float yaw = 0.0f, pitch = 0.0f;  // radians; yaw 0 looks toward -z? see forward()

    Vec3 eye() const { return {pos.x, pos.y + EYE_OFFSET, pos.z}; }
    Vec3 forward() const {
        float cp = std::cos(pitch);
        return {-std::sin(yaw) * cp, std::sin(pitch), -std::cos(yaw) * cp};
    }
};

// Advance one frame. move = (strafe right, forward) in [-1, 1]. Returns true if in water.
bool step_player(Player& p, const World& w, float dt, float strafe, float fwd, bool jump);

}  // namespace mc
