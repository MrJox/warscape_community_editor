# TEd's cameras

Static reading of `TEd.AssemblyKit.exe` (Attila kit) and its saved `Ted.xml`; TEd was not run.

- Modes (`Ted::ECAMERA_MODE`, menu labels in order): Look (`CM_LOOK`), Walk (`CM_WALK`), Orbit (`CM_ORBIT`), Ortho Top (`CM_TOPDOWN`).
- Per camera (`Ted::EDITOR_CAMERA`, `ModuleCamera.h`): `smoothed`, `movement_mode`, `ground_height`, `terrain_collision`, `head_bob`, `roll`,
  `orthographic`, `orthographic_height`, `DOF_focus`, `focal_length`, `DOF_blur_size`.
- Application settings (`Ted::APPLICATION_SETTINGS2`; the values in `Ted.xml` here): `camera_move_speed` 10, `camera_rotate_speed` 1,
  `camera_scale_speed_with_distance` true, `camera_collision_with_terrain` false, `camera_smoothing` true (on / off only), `load_camera`
  false.
- TEd collides only with the terrain; nothing keeps its camera out of buildings.

The game (Attila): `default_camera_type` in `preferences.script.txt` (0 totalwar, 1 rts, 2 debug), `camera_move_speed` 100,
`camera_turn_speed` 5; the `battle_cameras` table's land row: `min_height` 2, `max_height_small` 60, `max_height_large` 90,
`move_speed_min_multiplier` 0.15, `move_speed_max_multiplier` 1.5, `turn_speed_multiplier` 1.5 (mixed: min 4, naval: min 6.5).

The editor's cameras (`wce_editor_core::camera_controller`) take these as their models: Total War (the game's), Top-down (Ortho Top),
Walk, Fly (Look) and Orbit. Not ported: `head_bob`, `roll`, depth of field.
