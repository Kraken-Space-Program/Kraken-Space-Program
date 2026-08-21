part {
  id = "kraken.decoupler.small",
  author = "Claude",

  -- No mesh authored yet; the loader warns and substitutes a primitive.
  shape = { kind = "decoupler", radius = 0.625 },

  display_name = "TD-12 Stack Decoupler",
  manufacturer = "Seraphina Aerospace Industries",
  description = "Two metal rings and a shaped charge. SAI's legal department insists on describing it as a 'controlled separation event initiator', which is how you know it works.",
  categories = {"structural", "decoupler", "1.25m"},

  mass = 0.05,  -- tonnes, dry (the loader converts to kg)

  attach_nodes = {

    top = {
      position         = vec3(0, 0.1, 0),
      size             = 2,  -- 1.25 meter
      tensile_strength = 600.0,  -- kN before joint breaks under pull
      shear_strength   = 450.0,  -- kN before joint breaks under shear
    },

    bottom = {
      position         = vec3(0, -0.1, 0),
      size             = 2,
      tensile_strength = 600.0,
      shear_strength   = 450.0,
    },
  },

  modules = {
    decoupler {
      -- Fires when this stage number comes up. Lower numbers fire first.
      stage          = 0,
      -- The node released on firing. The other node stays attached, so the decoupler
      -- travels with the stage above it.
      node           = "bottom",
      ejection_force = 15.0,  -- kN impulse pushing the two halves apart
    },
  },
}
