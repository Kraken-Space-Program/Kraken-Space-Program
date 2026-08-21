-- A first-stage engine.
--
-- The stock Spark is a 20 kN upper-stage engine; a single 1.25 m tank fuelled with 2 t of
-- propellant weighs about 24 kN on the pad, so a Spark-only stack cannot lift itself and
-- never leaves the ground. This is the engine that makes the test stack fly: 1.25 m to
-- match the tank, and roughly 2:1 thrust-to-weight for the full two-stage vehicle.

part {
  id = "kraken.engine.reliant",
  author = "Claude",

  -- No mesh authored yet; the loader warns and substitutes a placeholder.
  shape = { kind = "engine", radius = 0.625 },

  display_name = "RE-M3 \"Reliant\" RP-1 engine",
  manufacturer = "Seraphina Aerospace Industries",
  description = "SAI's answer to the question 'what if the Spark, but it could actually lift something'. Gimbal-less, sea-level optimised, and famously indifferent to how it is treated.",
  categories = {"engine", "RP-1", "1.25m"},

  mass = 1.0,  -- tonnes, dry (the loader converts to kg)

  attach_nodes = {

    top = {
      position         = vec3(0, 0.5, 0),
      size             = 2,  -- 1.25 meter
      tensile_strength = 720.0,  -- kN before joint breaks under pull
      shear_strength   = 540.0,  -- kN before joint breaks under shear
    },

    bottom = {
      position         = vec3(0, -0.5, 0),
      size             = 2,
      tensile_strength = 540.0,
      shear_strength   = 405.0,
    },
  },

  modules = {
    engine {
      thrust      = 120,   -- kN, vacuum
      isp_vac     = 310,   -- s
      isp_sl      = 280,   -- s
      propellants = { RP1 = 0.3, LOX = 0.7 },
    },
  },
}
