part {
  id = "kraken.fueltank.small.03",
  author = "Graph",

  geometry = "fueltank_1.glb",  -- optional visual override; the shape below is what collides
  shape = { kind = "cylinder", radius = 0.625 },

  display_name = "RP-FT-200 Basic Small Fuel Tank (medium length)",
  manufacturer = "Seraphina Aerospace Industries",
  description = "Engineers at Seraphina Aerospace Industries were tasked with designing a fuel tank for use when running static tests of its RP-1 engines. They went a bit overboard, and SAI is now in the business of selling space-grade fuel tanks.",
  categories = {"fuel tank", "RP-1", "1.25m"},
  

  mass = 0.25,  -- metric tons, dry (the loader converts to kg)

  attach_nodes = {

    top = {
      position         = vec3(0, 1, 0),  -- mesh is centered at middle of tank
      size             = 2,  -- 1.25 meter
      tensile_strength = 720.0,  -- kN before joint breaks under pull
      shear_strength   = 540.0,  -- kN before joint breaks under shear
    },

    bottom = {
      position         = vec3(0, -1, 0),  -- mesh is centred at middle of tank
      size             = 2,  -- 1.25 meter
      tensile_strength = 720.0,  -- kN before joint breaks under pull
      shear_strength   = 540.0,  -- kN before joint breaks under shear
    },
  },
  
  modules = {
    -- Resource quantities are kilograms, unlike `mass` above which is tonnes.
    -- Both are normalised to kg at load; only these two units exist in the format.
    resource_container {
      name = RP1,
      amount = 600,  -- rocket propellent - 1, kilograms
      max = 600,  -- rocket propellent - 1, kilograms

    },

    resource_container {
      name = LOX,
      amount = 1400, -- LO2, kilograms
      max = 1400, -- LO2, kilograms
    },
  },
}