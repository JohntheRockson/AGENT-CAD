Agentic CAD MVP (Rust kernel + JS UI)

Honest take: is this a good idea? Can we beat existing projects?

The idea is good. Code-to-CAD with an agent loop is the only approach that currently produces editable mechanical parts instead of pretty meshes. Building it is a strong engineering-student project and can become a tool you actually use for homework and personal parts.

We will not beat Zoo by doing the same thing they do, only smaller. Zoo has a custom kernel, a custom language (KCL), Text-to-CAD since 2023, and Zookeeper shipping in Zoo Design Studio (Jan 2026). CADSmith, GrandpaCAD, and several GitHub MVPs already do prompt-to-CadQuery/OpenSCAD. A student MVP does not out-kernel or out-fund them.

Where we can still win a niche:





Cursor-like UX for mechanical students: chat + editable feature tree + live SolidWorks-style viewport, not a one-shot STL generator.



A tiny, schema-locked CAD language (JSON IR) that the model cannot hallucinate as easily as CadQuery's huge API.



A path to engineering software after CAD works: materials, load cases, simple FEA. Zoo and GrandpaCAD are not Fusion 360. That later chapter is the real differentiator — but it is not the MVP.

What would make this a bad idea: trying to certify a 10,000 lb hitch with safety factor 3 in v0, writing our own B-Rep kernel, or competing with Zoo on "conversational CAD" as the only pitch.

Locked product sequence:





Now: CAD only. Editable 3D solids from prompt / photo / dimensions. Like a skinny SolidWorks, driven by an agent.



Later: simulation (stress, safety factor, Fusion-style FEA). The hitch prompt becomes valid only in this phase.

Did you understand the architecture? Almost — one important correction

Your picture:



JS UI → prompt or picture → LLM → JSON → Rust → kernel → 3D

That is the right pipeline, with two fixes:





Rust does not draw the 3D view. The kernel builds an exact B-Rep solid (the real CAD model), tessellates it into triangles, and sends that mesh (plus STEP if exporting) to the browser. Three.js on the JavaScript frontend renders it. Rust is geometry + agent, not a game engine.



The LLM should not be a one-shot JSON dump. It writes JSON, Rust runs it, if the solid is invalid or a dimension is wrong the agent gets the error and retries. That loop is what makes it "agentic" instead of ChatGPT-with-an-STL.

Corrected flow for a prompt like "make a bracket to fit this part, here are dimensions" (and later, a photo of the mating part):

flowchart TD
  user["User: text and optional photo plus dimensions"] --> ui[JS chat plus editor plus viewport]
  ui --> server[Rust axum server]
  server --> llm[LLM interprets intent]
  llm --> json[JSON feature tree]
  json --> interp[Rust IR interpreter]
  interp --> occt[OpenCASCADE kernel]
  occt --> solid[B-Rep solid: the CAD model]
  solid --> mesh[Tessellated triangles]
  mesh --> ui
  solid --> step[STEP or STL export]
  occt --> metrics[Volume bbox is_solid]
  metrics --> llm

Trailer-hitch example: v0 may generate a shape that looks like a hitch if you describe the 2.5 inch ball and overall size. It will not know whether it holds 10,000 lb. Capacity and safety factor wait for the simulation phase.

Photos: useful for "fit this part." v0 can accept an image in chat so the vision model reads approximate size/features, but dimensions in the prompt still win. Do not treat a photo as a scan-to-CAD reverse-engineering product in the MVP.

Languages (unchanged)

JavaScript/TypeScript for the UI is a good call. Viewport is a browser canvas.

Rust is a good backend if we wrap OpenCASCADE. Wrong place to write B-Rep math or a GPU renderer. Do not have the LLM write Rust; it writes JSON, Rust interprets it.

North star UX: Zoo Zookeeper. Difference: we reuse OCCT, keep the language tiny, and leave a door open for FEA later.

What existing projects prove





Zoo Zookeeper (Jan 2026): agent writes KCL, executes against their kernel, inspects geometry with measurements and screenshots. Confirms B-Rep + code, not meshes.



CADSmith: generate CadQuery, run it, measure solids, feed failures back. The execute-validate-retry loop matters more than one-shot generation.



GrandpaCAD bake-off: OpenSCAD is easiest for LLMs (~0.4 errors/gen); CadQuery/Build123d fail more because the APIs are huge. Implication for us: keep our CAD language tiny.



vespo92/AI-CAD and zeemarquez/3dcad: React + Three.js + Replicad (OCCT in WASM) is the proven web CAD UI stack.



CadQueryEval (Aug 2026): top models ~84% on CadQuery tasks with geometry checks. Without a closed loop, first-try CAD code is unreliable.

Library research (what we would actually use)

CAD kernel (Rust) — pick one wrapper, do not write a kernel





Primary: occt-wasm (v3). OpenCASCADE V8 as a crate, no C++ toolchain. cargo add occt-wasm. Primitives, booleans, extrude/revolve, fillet/chamfer, tessellate, STEP/STL/glTF, volume/area/COM. Runs via wasmtime (extra isolation). Best fit for a Windows student MVP.



Backup: cadrum. Friendlier Solid API, prebuilt OCCT 8.0 for x86_64-pc-windows-msvc, native glTF/STL/STEP. Slightly heavier native link.



Skip for MVP: truck / monstertruck (pure Rust, incomplete industrial ops); opencascade-rs (needs system OCCT / cxx).

CAD language the AI writes

A JSON feature tree (schema-validated) in v0, not CadQuery and not raw Rust.

Example shape of the IR:

{
  "units": "mm",
  "features": [
    { "op": "sketch", "id": "s1", "plane": "XY", "profile": { "rect": { "w": 40, "h": 20 } } },
    { "op": "cut", "profile": { "circle": { "d": 6, "at": [10, 10] } } },
    { "op": "extrude", "id": "body", "depth": 5 },
    { "op": "fillet", "edges": "all", "radius": 1 }
  ]
}

Why JSON: LLMs can emit it via structured output; we can reject invalid ops before touching OCCT; we can show it in the editor; later we can add a prettier DSL on top. Keep the op set small so the model does not hallucinate methods (the CadQuery failure mode).

v0 ops: sketch (rect, circle, line-loop, arc), extrude, revolve, boolean cut/fuse, hole, fillet, chamfer, transform. No assemblies, no full 2D constraint solver yet.

Frontend (JavaScript ecosystem)

Use TypeScript + React (still the JS UI you wanted; types will save us on mesh/IR payloads).





Vite + React — app shell



Three.js + @react-three/fiber + drei — orbit viewport, grid, edges (same as AI-CAD / 3dcad)



Monaco Editor — editable CAD program, like Cursor



Zustand — chat + model + mesh state



Tailwind — layout (chat | code | viewport)

The backend sends a triangle mesh (or glTF) plus edge lines. The browser does not run OCCT.

Agent / HTTP (Rust)





axum + tokio — API and SSE streaming



async-openai (or Anthropic SDK) — tool-calling agent. Keep this boring; do not adopt a 0.1 LLM framework as a dependency of the CAD product.



Agent tools (the Cursor part): write_program, run_model, measure (volume, bbox, area, is_solid), list_topology (faces/edges for fillets), export_step / export_stl



Loop: write IR → execute → on kernel/syntax error, feed the exact error back → optional measure vs requested dimensions → stop when solid is valid or retries exhausted

Sandbox: the CAD program is data, not executable OS code, so we avoid Python exec(). Still timeout OCCT calls; booleans can hang.

Repo layout (empty workspace today)

apps/web/          # Vite React TS UI
crates/kernel/     # occt-wasm wrapper + IR interpreter
crates/agent/      # LLM tool loop
crates/server/     # axum: /chat, /run, /export

MVP scope (and what to cut)

In (CAD, like a skinny SolidWorks):





Chat (text; optional photo for context) → agent writes JSON IR → 3D preview updates



Manual edit of the IR + Run without another LLM call



Sketch / extrude / cut / fillet / chamfer on a single part



Export STL and STEP



Kernel validity + bbox/volume in the retry loop

Out of v0:





Load capacity, safety factor, materials, FEA (Fusion-style simulation is phase 2)



Assemblies, drawings, CAM



Full sketch constraint solver (SolveSpace WASM can come later)



Point-and-click modeling



Writing Truck/our own kernel



Rust GPU rendering (the browser draws the mesh)

Phase 2 (after CAD is actually good): attach materials and load cases to the same B-Rep, run a simple linear-static solver, and only then take prompts like the 10,000 lb hitch seriously. The JSON IR and OCCT solid are the bridge; we do not throw the CAD away to start over.

Honest risks





LLM reliability on a custom IR is the main risk (no CadQuery-sized training corpus). Mitigate with a tiny schema, few-shot examples in the system prompt, and the execute/measure loop.



Fillet edge IDs are the classic CAD-as-code pain (CascadeStudio). v0: fillet all edges or edges-by-length/index from list_topology, not mouse-picked IDs.



occt-wasm first-load is ~21 MB decompressed in-process; fine for a local server, not for shipping the kernel to the browser.

Implementation order after you approve





Kernel crate: box, sketch-rect, extrude, cut-circle, fillet, tessellate, STEP — proven with unit tests, no UI.



JSON IR → those ops.



axum /run and /export.



Web UI: viewport + editor + run button (hardcoded IR first).



Chat agent with tools and retry.



Tighten prompts with 5–10 student parts (phone stand, L-bracket, spacer, flange).

