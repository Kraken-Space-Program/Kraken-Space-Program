part {
  id = "kraken.pod.probe",
  author = "Claude",
  shape = { kind = "cylinder", radius = 0.4 },
  display_name = "PC-1 \"Sightline\" Probe Core",
  manufacturer = "Seraphina Aerospace Industries",
  description = "A guidance computer, a battery and three reaction wheels in a hockey puck. SAI will not disclose the clock speed, but insiders report it is 'sufficient, provided nothing unexpected happens'.",
  categories = {"command", "probe", "0.625m"},
  mass = 0.04,  -- tonnes, dry (the loader converts to kg)
  attach_nodes = {
    bottom = {
      position         = vec3(0, -0.15, 0),
      size             = 2,  -- 1.25 meter
      tensile_strength = 240.0,  -- kN before joint breaks under pull
      shear_strength   = 180.0,  -- kN before joint breaks under shear
    },
  },
  modules = {
    -- kN·m at full deflection. Matches the global constant this replaced, so the test
    -- stack still handles the way it did when the number was hardcoded.
    reaction_wheel { torque = 3.0 },
  },
}
