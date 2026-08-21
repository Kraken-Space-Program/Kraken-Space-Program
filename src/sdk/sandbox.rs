//! Lua VM setup and the tick budget mechanism.
//!
//! design.md, "What Lua cannot do": mods get no filesystem, no network, no process
//! control, and no ability to hang the game. That is enforced here, at VM construction,
//! rather than by asking mod authors nicely.

use mlua::{HookTriggers, Lua, StdLib, Table, VmState};

/// Instructions between budget checks.
///
/// The hook itself costs something, so checking every instruction would be absurd; 10 000
/// is frequent enough to catch a runaway loop within microseconds and rare enough to be
/// free in normal code.
const INSTRUCTION_CHECK_INTERVAL: u32 = 10_000;

/// Creates a sandboxed Lua state.
///
/// # What is available
///
/// `string`, `table`, `math` — everything a declarative part or planet definition needs.
///
/// # What is not, and why
///
/// - `io`, `os` — filesystem and process access. A part definition has no business
///   reading `/etc/passwd`, and "mod authors are trustworthy" is not a security model.
/// - `package` — `require` would let a mod escape the sandbox by loading a C module.
/// - `debug` — can defeat every other restriction here, including the instruction hook.
/// - `coroutine` — not needed by definitions, and it complicates the tick budget, which
///   counts instructions per *thread*. It can be added deliberately when the SDK grows
///   scripted behaviour.
pub fn create_lua_vm() -> mlua::Result<Lua> {
    Lua::new_with(
        StdLib::STRING | StdLib::TABLE | StdLib::MATH,
        mlua::LuaOptions::default(),
    )
}

/// Installs the per-script instruction budget.
///
/// EXECUTION.md's Decision Log fixes the mechanism ("instruction count hook") and treats
/// the number as provisional. `max_instructions` is therefore a parameter, not a constant:
/// the load-time budget for a part definition and the per-tick budget for a running
/// script are different problems that happen to share this machinery.
///
/// An over-budget script is *killed*, not throttled. A mod that takes too long produces
/// an error the author can read; it never becomes everyone else's frame drop.
pub fn install_instruction_budget(lua: &Lua, max_instructions: u64) -> mlua::Result<()> {
    // `Cell` because mlua takes the hook as `Fn`, not `FnMut` — it can be re-entered from
    // nested Lua calls, so it cannot hand out a unique borrow of the counter.
    let executed = std::cell::Cell::new(0u64);

    lua.set_hook(
        HookTriggers::new().every_nth_instruction(INSTRUCTION_CHECK_INTERVAL),
        move |_lua, _debug| {
            executed.set(executed.get() + u64::from(INSTRUCTION_CHECK_INTERVAL));
            if executed.get() > max_instructions {
                return Err(mlua::Error::runtime(format!(
                    "script exceeded its budget of {max_instructions} instructions — \
                     likely an infinite loop"
                )));
            }
            Ok(VmState::Continue)
        },
    )
}

/// Builds the environment table a definition file runs inside.
///
/// # The bare-identifier trick
///
/// The existing part files are written like this:
///
/// ```lua
/// resource_container { name = LOX, amount = 1400 }
/// propellants = { RP1 = 0.3, LOX = 0.7 }
/// ```
///
/// `LOX` and `RP1` are undefined globals — in stock Lua they evaluate to `nil`, and the
/// tank would silently load a container of nothing. Rather than force authors to quote
/// every resource name, the environment's `__index` returns the *name of the key* for
/// anything it does not recognise, so `LOX` evaluates to the string `"LOX"`.
///
/// The cost is that a typo becomes a string rather than an error: `mass = tonne` yields
/// `"tonne"`, which fails later with a type error naming the field. That is an acceptable
/// trade for a declarative format — and the alternative, `nil`, fails *silently*, which is
/// strictly worse.
pub fn create_definition_environment(lua: &Lua) -> mlua::Result<Table> {
    let env = lua.create_table()?;

    // Safe stdlib subset, copied in explicitly so the environment is a closed set.
    let globals = lua.globals();
    for name in [
        "string", "table", "math", "pairs", "ipairs", "type", "tostring",
    ] {
        if let Ok(value) = globals.get::<mlua::Value>(name) {
            env.set(name, value)?;
        }
    }

    let metatable = lua.create_table()?;
    metatable.set(
        "__index",
        lua.create_function(|_, (_table, key): (Table, String)| Ok(key))?,
    )?;
    env.set_metatable(Some(metatable))?;

    Ok(env)
}
