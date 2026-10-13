# ButureCAD — Roadmap de Desenvolvimento para Codex 6 Astra

## Visão do projeto

O **ButureCAD** será um software profissional de Electronic Design Automation (EDA) para criação de esquemáticos e placas de circuito impresso.

O objetivo de longo prazo é oferecer um fluxo completo:

Schematic → ERC → Footprints → PCB → Placement → Routing → DRC → 3D → Manufacturing

Além do fluxo tradicional de EDA, o ButureCAD terá:

- funcionamento 100% local;
- funcionamento offline;
- arquitetura leve;
- Rust no core;
- renderização GPU;
- editor de esquemáticos;
- editor de PCB;
- roteamento de trilhas;
- vias;
- multilayer;
- differential pairs;
- length matching;
- copper zones;
- ERC;
- DRC;
- bibliotecas;
- plugins;
- visualização 3D;
- fabricação;
- SPICE;
- linguagem própria chamada ButureScript;
- CLI;
- funcionamento headless;
- Agent API;
- integração opcional com agentes de IA.

## Regras gerais para TODOS os prompts

Estas regras permanecem válidas durante todo o desenvolvimento:

1. O nome do software é **ButureCAD**.
2. O programa deve funcionar completamente offline.
3. Não adicionar cloud obrigatória.
4. Não adicionar Supabase.
5. Não adicionar autenticação.
6. Não adicionar login.
7. Não adicionar contas.
8. Não adicionar servidor remoto obrigatório.
9. Não adicionar telemetria obrigatória.
10. Os arquivos pertencem completamente ao usuário.
11. React deve cuidar principalmente da interface.
12. Operações pesadas devem permanecer no core Rust.
13. PCB e schematic não devem depender do DOM para renderização pesada.
14. Utilizar GPU quando apropriado.
15. Não copiar código proprietário ou incompatível de outros EDAs.
16. Verificar licenças antes de adicionar dependências.
17. Preservar funcionalidades implementadas em prompts anteriores.
18. Não substituir implementações funcionais por mocks.
19. Não criar botões falsos apenas para parecer que uma feature existe.
20. Sempre executar build/testes após alterações importantes.
21. Corrigir erros introduzidos antes de prosseguir.
22. Priorizar arquitetura modular.
23. Priorizar performance.
24. Priorizar baixo consumo de memória.
25. Priorizar confiabilidade.
26. Priorizar precisão necessária para CAD/EDA.
27. Toda alteração destrutiva deve passar pela Command Engine.
28. Não declarar uma feature como pronta sem implementação funcional.
29. Manter documentação técnica atualizada.
30. Quando houver dúvida arquitetural, escolher a solução mais sustentável para um EDA profissional de longo prazo.

---

# PROMPT 01 — Fundação do ButureCAD

Atue como arquiteto de software sênior especializado em EDA/CAD, eletrônica, PCB, Rust, computação gráfica, geometria computacional e aplicações desktop.

Vamos construir do zero um software chamado:

**BUTURECAD**

O ButureCAD será um EDA profissional para projeto eletrônico e PCB.

## Objetivo

Permitir realizar localmente:

Schematic
→ ERC
→ Footprints
→ PCB
→ Placement
→ Routing
→ DRC
→ 3D
→ Manufacturing

Além disso teremos:

- design por código;
- automação;
- plugins;
- integração opcional com agentes de IA;
- CLI;
- headless mode.

## Restrições fundamentais

- 100% local;
- nenhuma conta;
- nenhum login;
- nenhuma cloud;
- nenhum Supabase;
- nenhum servidor obrigatório;
- nenhuma telemetria obrigatória;
- nenhuma dependência obrigatória de internet;
- projetos pertencem completamente ao usuário;
- aplicação deve continuar funcionando indefinidamente offline.

## Stack

Desktop:

- Tauri 2
- React
- TypeScript
- Vite

Core:

- Rust

Rendering:

- wgpu

Storage:

- arquivos locais;
- SQLite somente para cache/indexação quando realmente necessário.

React deve cuidar principalmente da UI.

PCB, schematic, geometria, seleção, DRC, ERC, routing e operações pesadas devem permanecer no Rust.

Crie workspace modular:

apps/
  desktop/

crates/
  eda-core/
  geometry/
  renderer/
  schematic/
  pcb/
  router/
  rules/
  libraries/
  manufacturing/
  simulation/
  viewer3d/
  io/
  scripting/
  plugin-runtime/
  agent-api/

Crie:

docs/ARCHITECTURE.md
docs/ROADMAP.md
docs/DEPENDENCIES.md

Não copie código de KiCad, Altium ou outros EDA.

Antes de adicionar dependências, verifique se a licença é adequada ao modelo de distribuição comercial proprietário.

Configure:

- build;
- lint;
- format;
- tests;
- CI.

Implemente apenas a fundação neste momento.

Ao terminar:

1. compile tudo;
2. rode os testes;
3. corrija warnings relevantes;
4. documente o estado atual.

---

# PROMPT 02 — Formato de projeto

Continue o ButureCAD.

Crie o formato de projeto local.

Quero projetos portáveis e amigáveis ao Git.

Estrutura conceitual:

MyBoard/
  project.buture
  schematic/
  pcb/
  libraries/
  models/
  scripts/
  rules/
  output/

O formato deve ser:

- determinístico;
- versionável;
- documentado;
- human-readable quando razoável;
- extensível.

Implemente:

- New Project;
- Open Project;
- Save;
- Save As;
- autosave;
- backup;
- crash recovery.

Use gravações atômicas para evitar corrupção.

Adicione schema versioning e migrations.

Crie testes para:

- salvar;
- carregar;
- arquivo incompleto;
- arquivo inválido;
- versões antigas.

Documente em:

docs/file-format/

---

# PROMPT 03 — Command Engine

Implemente a Command Engine do ButureCAD.

Nenhuma alteração importante no projeto deverá modificar o modelo diretamente.

Use Command Pattern.

Precisamos de:

- Undo;
- Redo;
- transaction groups;
- history;
- dirty state;
- autosave integration;
- recovery.

Exemplos:

PlaceComponentCommand
MoveComponentCommand
DeleteComponentCommand
CreateWireCommand
CreateTrackCommand
CreateViaCommand

Todas as futuras ferramentas deverão utilizar essa infraestrutura.

Crie testes extensivos.

Não implemente hacks temporários.

---

# PROMPT 04 — Motor de geometria

Crie o geometry engine do ButureCAD em Rust.

Precisamos de primitivas robustas:

- Point;
- Vector;
- Line;
- Segment;
- Arc;
- Circle;
- Rectangle;
- Polygon;
- BoundingBox.

Implemente operações:

- intersection;
- distance;
- nearest point;
- containment;
- collision;
- offset;
- rotation;
- translation;
- bounding boxes.

Use unidades internas apropriadas para CAD e evite erros acumulativos de floating point.

Defina claramente o sistema de coordenadas.

Implemente spatial indexing.

Considere R-tree, BVH ou estrutura apropriada após análise.

Crie property-based tests para geometria.

Esse módulo será crítico para PCB, routing e DRC.

---

# PROMPT 05 — Renderer GPU

Crie o renderer 2D usando wgpu.

Não renderize PCB usando milhares de elementos DOM/SVG.

Implemente:

- pan;
- zoom;
- grid;
- adaptive grid;
- snap;
- selection;
- hover;
- box selection.

Primitivas:

- lines;
- arcs;
- circles;
- rectangles;
- polygons;
- text.

Implemente layers.

O renderer deve conseguir lidar futuramente com centenas de milhares de objetos.

React controla UI.

Rust/wgpu controla canvas.

Implemente picking eficiente.

Adicione FPS/debug overlay opcional para desenvolvimento.

---

# PROMPT 06 — Shell profissional

Construa a interface principal do ButureCAD.

Quero aparência de ferramenta CAD/engenharia profissional, não dashboard SaaS.

Estrutura:

- Top Toolbar;
- Left Tool Palette;
- Document Tabs;
- Central Canvas;
- Right Properties Inspector;
- Bottom Panel.

Bottom Panel poderá mostrar:

- ERC;
- DRC;
- Messages;
- Console.

Modos:

- Schematic;
- PCB;
- Symbol Editor;
- Footprint Editor;
- 3D Viewer.

Adicione:

- dark mode;
- light mode;
- command palette;
- keyboard shortcuts;
- context menus;
- status bar.

Status bar deve mostrar:

- cursor X/Y;
- grid;
- zoom;
- units;
- active layer.

Não implemente funcionalidades falsas.

---

# PROMPT 07 — Schematic Core

Implemente o modelo de dados do Schematic Editor.

Entidades:

- Component;
- Symbol;
- Pin;
- Wire;
- Bus;
- Junction;
- NetLabel;
- GlobalLabel;
- PowerSymbol;
- Sheet.

Suporte:

- hierarchical sheets;
- component references;
- values;
- properties;
- net connectivity.

Separe rigorosamente:

- data model;
- rendering;
- tools;
- electrical connectivity.

---

# PROMPT 08 — Editor esquemático

Transforme o Schematic Core em um editor utilizável.

Implemente ferramentas:

- Place Component;
- Wire;
- Bus;
- Junction;
- Net Label;
- Global Label;
- Power Symbol;
- Move;
- Drag;
- Rotate;
- Mirror;
- Delete;
- Duplicate.

Implemente:

- multi-selection;
- copy/paste;
- box selection;
- snap;
- keyboard shortcuts.

Wires devem manter conectividade corretamente durante movimentações.

Cruzamentos visuais não devem automaticamente representar conexão.

---

# PROMPT 09 — Symbol Editor

Crie o Symbol Editor.

Permita criar símbolos eletrônicos contendo:

- pins;
- pin names;
- pin numbers;
- electrical types;
- lines;
- arcs;
- rectangles;
- circles;
- text.

Pin types:

- input;
- output;
- bidirectional;
- tri-state;
- passive;
- power input;
- power output;
- open collector;
- open emitter;
- no connect.

Suporte:

- multiple units;
- alternate representations;
- metadata.

Salve símbolos em bibliotecas locais.

---

# PROMPT 10 — Connectivity Engine

Crie uma connectivity engine independente.

Construa um grafo elétrico a partir do esquemático.

Precisamos descobrir:

- quais pins pertencem a cada net;
- junctions;
- labels;
- global labels;
- power nets;
- hierarchical connections.

Atualizações pequenas não devem obrigatoriamente reconstruir o grafo inteiro.

Crie testes para circuitos complexos.

---

# PROMPT 11 — ERC

Implemente Electrical Rules Check.

Detecte inicialmente:

- unconnected pins;
- output-output conflicts;
- power input without source;
- duplicate references;
- invalid connections;
- dangling wires;
- missing component data.

Classifique:

- Error;
- Warning;
- Info.

Cada problema deve ser clicável.

Ao clicar:

- centralize;
- destaque o elemento no schematic.

Permita exceptions/waivers documentadas.

---

# PROMPT 12 — Footprint Editor

Crie o Footprint Editor.

Elementos:

- SMD pads;
- THT pads;
- NPTH holes;
- custom pads;
- silkscreen;
- fabrication;
- courtyard;
- solder mask;
- paste;
- reference/value text.

Pad shapes:

- circle;
- rectangle;
- oval;
- rounded rectangle;
- custom.

Adicione generators paramétricos iniciais:

- DIP;
- SOIC;
- QFN;
- BGA;
- headers.

Permita associar modelos 3D futuramente.

---

# PROMPT 13 — PCB Core

Implemente o modelo central do PCB.

Entidades:

- Board;
- Footprint;
- Pad;
- Track;
- Via;
- Zone;
- Keepout;
- Text;
- Graphic;
- Net;
- Layer.

Layers iniciais:

- Front Copper;
- Back Copper;
- Inner Copper;
- Front/Back Mask;
- Front/Back Paste;
- Front/Back Silkscreen;
- Edge Cuts;
- Mechanical/User layers.

Prepare arquitetura para placas multilayer.

Todas as coordenadas devem utilizar o geometry engine.

---

# PROMPT 14 — PCB Editor

Construa o PCB Editor visual.

Implemente:

- board outline;
- footprint placement;
- move;
- rotate;
- flip;
- align;
- distribute;
- lock;
- delete;
- duplicate;
- multi-selection.

Adicione Layer Manager.

Permita esconder/mostrar layers.

Implemente active layer.

Pads, footprints, board outline e elementos gráficos devem ser renderizados via wgpu.

---

# PROMPT 15 — Schematic → PCB

Implemente:

**Update PCB from Schematic**

Mapeie:

- components;
- footprints;
- nets;
- references.

Detecte:

- added components;
- removed components;
- changed footprints;
- changed nets.

Antes de aplicar alterações, mostre preview.

Não destrua placement ou routing existente desnecessariamente.

Utilize a Command Engine para aplicar alterações.

---

# PROMPT 16 — Ratsnest

Implemente ratsnest/flying wires.

Mostre visualmente conexões ainda não roteadas.

Atualize dinamicamente durante:

- placement;
- routing;
- movement.

Não recalcule toda a placa desnecessariamente.

Use algoritmos e estruturas espaciais adequadas.

Adicione:

Show/Hide Ratsnest.

---

# PROMPT 17 — Trilhas

Implemente PCB Track Routing básico.

O usuário deve conseguir clicar em um pad e desenhar uma trilha até outro pad.

Suporte:

- 45 degree routing;
- 90 degree routing;
- free angle quando apropriado;
- track width;
- active copper layer;
- snap;
- finish route;
- cancel route.

Durante routing:

- mostrar preview;
- mostrar clearance;
- destacar net;
- mostrar destino.

Tracks devem pertencer corretamente às nets.

Permita editar posteriormente:

- width;
- segments;
- vertices;
- net.

Integre tudo à Command Engine para Undo/Redo.

---

# PROMPT 18 — Vias e troca de layer

Implemente vias.

Durante routing o usuário deve conseguir inserir uma via e mudar de copper layer.

Suporte inicialmente:

- through vias.

Propriedades:

- diameter;
- drill;
- net.

Prepare arquitetura para:

- blind vias;
- buried vias;
- microvias.

A mudança de layer durante routing deve ser fluida.

Atualize ratsnest automaticamente.

---

# PROMPT 19 — DRC Engine

Implemente Design Rule Checking.

Primeiras regras:

- minimum clearance;
- minimum track width;
- via diameter;
- via drill;
- copper-to-edge;
- pad clearance;
- track collision;
- short circuits;
- unconnected nets;
- courtyard overlap.

Crie arquitetura incremental.

DRC violations devem aparecer:

- no PCB;
- no painel DRC.

Clique em um erro:

- zoom no problema;
- highlight dos objetos envolvidos.

---

# PROMPT 20 — Net Classes

Implemente Net Classes.

Exemplos:

- Default;
- Power;
- USB;
- Differential;
- HighCurrent.

Cada classe poderá definir:

- track width;
- clearance;
- via size;
- via drill;
- differential pair gap;
- differential pair width.

Permita associar nets às classes.

Router e DRC devem utilizar essas regras automaticamente.

---

# PROMPT 21 — Router Walk Around

Evolua o interactive router.

Implemente modo:

**WALK AROUND**

Enquanto o usuário roteia uma track, o sistema deve detectar obstáculos e calcular caminhos válidos ao redor deles.

Obstáculos:

- pads;
- tracks;
- vias;
- zones;
- keepouts;
- board boundaries.

Sempre respeite clearance.

Mostre preview em tempo real.

Separe:

- path finding;
- geometry;
- collision;
- DRC validation.

---

# PROMPT 22 — Push & Shove

Implemente modo:

**PUSH & SHOVE**

Durante routing, uma nova trilha poderá deslocar tracks/vias existentes quando houver solução válida.

Nunca gere silenciosamente geometria inválida.

O resultado deve respeitar DRC.

Adicione:

- Walk Around;
- Push & Shove;
- Ignore Obstacles.

como modos selecionáveis.

Crie cenários automatizados de teste.

---

# PROMPT 23 — Differential Pairs

Implemente Differential Pair Routing.

Permita definir pares como:

USB_D+ / USB_D-

ETH_TX+ / ETH_TX-

Configure:

- width;
- gap;
- maximum skew.

Roteie as duas tracks conjuntamente.

Mostre em tempo real:

- length +;
- length -;
- difference;
- gap.

Integre às Net Classes.

---

# PROMPT 24 — Length Matching

Implemente length tuning.

O usuário poderá selecionar uma net ou differential pair e definir:

- target length;
- tolerance.

Implemente:

- serpentine;
- accordion/meander.

Mostre:

- Current;
- Target;
- Difference.

Atualize em tempo real.

---

# PROMPT 25 — Copper Zones

Implemente Copper Zones / Polygon Pour.

Suporte:

- net assignment;
- clearance;
- thermal relief;
- solid connection;
- priority;
- keepouts;
- island removal.

Implemente refill.

Exemplo:

GND plane cobrindo uma copper layer.

O algoritmo deve funcionar eficientemente em placas grandes.

---

# PROMPT 26 — Stackup

Crie Layer Stack Manager.

Configure:

- copper layers;
- copper thickness;
- dielectric thickness;
- material;
- core;
- prepreg;
- layer ordering.

Prepare arquitetura para:

- 2 layer;
- 4 layer;
- 6 layer;
- 8+ layer;
- HDI;
- rigid-flex.

Permita salvar stackups como presets locais.

---

# PROMPT 27 — PCB 3D

Implemente 3D Viewer.

Renderize:

- board substrate;
- solder mask;
- copper;
- pads;
- vias;
- silkscreen;
- components.

Suporte modelos 3D de componentes.

Controles:

- orbit;
- pan;
- zoom;
- perspective;
- orthographic.

Mudanças no PCB devem refletir no modelo 3D.

Mantenha viewer desacoplado do PCB Core.

---

# PROMPT 28 — STEP / MCAD

Adicione suporte arquitetural a STEP/MCAD.

Antes de escolher biblioteca, analise cuidadosamente licença e compatibilidade comercial.

Precisamos futuramente importar/exportar:

- board outline;
- mounting holes;
- component geometry;
- mechanical constraints.

Não acople o PCB Core diretamente à biblioteca STEP escolhida.

Crie uma abstraction layer.

---

# PROMPT 29 — Manufacturing

Implemente Manufacturing Export.

Gerar:

- Gerber;
- Excellon Drill;
- BOM;
- Pick and Place;
- assembly information.

Crie Manufacturing Preview.

Antes da exportação execute:

- DRC;
- board validation;
- manufacturing preflight.

Mostre claramente warnings.

Permita gerar:

production.zip

contendo arquivos necessários para fabricação/montagem.

---

# PROMPT 30 — Gerber Viewer

Crie Gerber Viewer integrado.

IMPORTANTE:

Ele deve interpretar os arquivos Gerber gerados, e não simplesmente mostrar novamente o modelo interno do PCB.

Isso permite verificar se a exportação realmente corresponde ao projeto.

Mostre:

- layers;
- drills;
- apertures.

Permita ligar/desligar layers.

---

# PROMPT 31 — Bibliotecas locais

Crie Library Manager completamente local.

Tipos:

- Symbol Libraries;
- Footprint Libraries;
- 3D Model Libraries;
- Component Libraries.

Component deve agregar:

- symbol;
- footprint;
- 3D model;
- manufacturer;
- MPN;
- description;
- datasheet;
- parameters.

Implemente:

- search;
- filter;
- favorites;
- recent components.

Indexação deve ser local e extremamente rápida.

---

# PROMPT 32 — Plugin System

Agora transforme plugins em uma feature central.

Tipos:

- ComponentProvider;
- SymbolProvider;
- FootprintProvider;
- Model3DProvider;
- Importer;
- Exporter;
- RulePlugin;
- ToolPlugin;
- SimulationPlugin.

Investigue WASM/WASI para sandbox.

Plugin NÃO recebe acesso arbitrário ao sistema.

Permissões como:

- filesystem;
- network;
- project-read;
- project-write.

devem ser declaradas explicitamente.

Crie:

- Plugin Manager;
- Install from file;
- Enable;
- Disable;
- Remove;
- Reload.

Tudo deve funcionar offline.

---

# PROMPT 33 — Component Provider API

Implemente ComponentProvider API.

Interface conceitual:

search(query)
get_component()
get_symbol()
get_footprint()
get_model()
get_datasheet()
get_metadata()

Isso permitirá plugins para bibliotecas e fornecedores.

IMPORTANTE:

ButureCAD Core não conhece nenhum fornecedor específico.

Plugins locais funcionam offline.

Plugins que acessarem internet precisam declarar:

network permission.

O usuário deve poder revogar essa permissão.

---

# PROMPT 34 — SPICE

Implemente Simulation Architecture.

Não desenvolva necessariamente um SPICE solver do zero.

Procure solução madura cuja licença seja compatível com nosso projeto.

Crie abstraction layer.

Suporte inicialmente:

- Operating Point;
- DC Sweep;
- AC;
- Transient.

Resultados devem aparecer em gráficos dentro do ButureCAD.

Schematic deve permitir associar modelos SPICE aos componentes.

---

# PROMPT 35 — ButureScript

Crie uma linguagem declarativa chamada:

**ButureScript**

Extensão:

`.beda`

Objetivo:

criar eletrônica através de código.

Precisamos representar:

- components;
- connections;
- nets;
- footprints;
- board;
- placement;
- tracks;
- vias;
- zones;
- layers;
- rules.

Exemplo apenas conceitual:

component MCU {
    part = "STM32F411"
}

component C1 {
    value = "100nF"
    footprint = "0402"
}

connect MCU.VDD -> C1.1
connect C1.2 -> GND

Analise e projete uma gramática adequada antes de implementar.

Crie:

- lexer;
- parser;
- AST;
- semantic analysis;
- diagnostics;
- compiler.

Pipeline:

ButureScript
↓
AST
↓
Validation
↓
EDA IR
↓
ButureCAD Core

Não acople linguagem ao renderer.

---

# PROMPT 36 — ButureScript paramétrico

Expanda ButureScript.

Adicione:

- variables;
- functions;
- modules;
- loops;
- conditions;
- parameters.

Adicione unidades ao type system:

- nm;
- um;
- mm;
- mil;
- inch;
- V;
- mV;
- A;
- mA;
- ohm;
- kOhm;
- MOhm;
- Hz;
- kHz;
- MHz;
- GHz.

Operações dimensionalmente inválidas devem gerar erro.

Exemplo:

10mm + 5V

deve resultar em:

ERROR: incompatible dimensions.

Permita design paramétrico.

Exemplo conceitual:

for i in 0..8 {
    place LED[i] at(i * 2.54mm, 20mm)
}

---

# PROMPT 37 — Code ↔ Visual

Integre ButureScript ao editor.

Crie:

- CODE MODE;
- VISUAL MODE;
- SPLIT MODE.

Split:

┌──────────────┬──────────────────┐
│ ButureScript │ PCB/Schematic    │
│              │                  │
│ código       │ visualização     │
│              │                  │
└──────────────┴──────────────────┘

Mudança no código:

parse
↓
validate
↓
EDA IR
↓
preview

Mostre erros com:

- line;
- column;
- message.

Implemente inicialmente:

Code → Visual.

Projete arquitetura para futuramente suportar:

Visual → Code

sem destruir código manual do usuário.

---

# PROMPT 38 — CLI

Crie:

**buturecad-cli**

O CLI deve utilizar exatamente o mesmo Rust Core.

Comandos:

buturecad build
buturecad check
buturecad erc
buturecad drc
buturecad export-gerber
buturecad export-bom

Também:

buturecad inspect components
buturecad inspect nets
buturecad inspect board

Outputs:

- human readable;
- JSON.

Todo fluxo crítico deve funcionar sem GUI.

---

# PROMPT 39 — Headless EDA

Garanta que o ButureCAD Core seja completamente headless.

Precisamos conseguir executar:

ButureScript
↓
Parser
↓
EDA Core
↓
Schematic
↓
PCB
↓
ERC
↓
DRC
↓
Gerber

sem:

- React;
- Tauri;
- janela gráfica.

A GUI deve ser cliente do Core.

Isso será essencial para:

- scripts;
- testes;
- automação;
- CI;
- plugins;
- agentes de IA.

---

# PROMPT 40 — Agent API

Crie ButureCAD Agent API.

Objetivo:

permitir que agentes de IA compreendam e modifiquem projetos.

READ:

get_project
get_schematic
get_board
get_components
get_nets
get_rules
get_violations

ACTIONS:

create_component
remove_component
connect
disconnect
place
move
route
create_via
create_zone
change_rule

ANALYSIS:

run_erc
run_drc
measure_net
measure_track

Nunca permita:

AI → alteração arbitrária dos arquivos.

Fluxo:

Agent
↓
Agent API
↓
Proposed Changes
↓
Validation
↓
Diff
↓
User Approval
↓
Command Engine

Todas as operações devem produzir representação estruturada e auditável.

---

# PROMPT 41 — AI Workspace

Crie AI Workspace.

A IA NÃO fará parte obrigatória do ButureCAD.

O ButureCAD apenas fornece uma interface para agentes.

Painel:

AI Workspace

Usuário pode solicitar:

"Adicione desacoplamento no STM32."

Agente poderá propor:

+ C14 100nF
+ C15 100nF
+ C16 4.7uF

Connections:

C14 → VDD/GND
C15 → VDD/GND

Placement:

near U1 power pins

Antes de aplicar:

- mostrar diff;
- executar ERC/DRC;
- mostrar warnings.

Botões:

- Apply;
- Reject;
- Inspect.

Arquitetura deve permitir futuramente conectar diferentes modelos locais ou externos sem alterar o EDA Core.

---

# PROMPT 42 — Plugin/Agent Protocol

Crie um protocolo estruturado para comunicação entre ButureCAD e ferramentas externas.

Objetivo:

um agente externo deve conseguir:

- inspecionar projeto;
- buscar componentes;
- executar ERC/DRC;
- propor mudanças;
- gerar ButureScript.

Sem acessar diretamente arquivos internos.

Considere JSON-RPC ou protocolo semelhante.

Projete também compatibilidade futura com ecossistemas de ferramentas para agentes.

Documente completamente em:

docs/agent-api/

Inclua exemplos em:

- Rust;
- TypeScript;
- Python.

---

# PROMPT 43 — Importação

Crie arquitetura modular de importadores.

Queremos futuramente importar projetos de outras ferramentas.

Não copie parsers de projetos incompatíveis com nossa licença.

Implemente somente formatos cuja documentação/licença permita nossa implementação.

Todo import deve gerar:

**Import Report**

contendo:

- converted;
- ignored;
- unsupported;
- warnings;
- errors.

Nunca descarte silenciosamente informação de projeto.

---

# PROMPT 44 — Performance pesada

Faça profiling profissional.

Gere benchmarks:

Small:
100 componentes

Medium:
500 componentes

Large:
2000 componentes

Extreme:
5000+ componentes

Teste:

- startup;
- project load;
- save;
- schematic rendering;
- PCB rendering;
- zoom;
- pan;
- selection;
- ratsnest;
- routing;
- DRC;
- ERC;
- zone refill.

Meça:

- CPU;
- RAM;
- FPS;
- latency.

Identifique gargalos reais.

Somente faça otimizações comprovadas por benchmark.

Registre before/after.

---

# PROMPT 45 — Robustez

Faça uma rodada dedicada exclusivamente à robustez.

Adicione:

- unit tests;
- integration tests;
- property tests;
- fuzzing.

Principalmente:

- file parser;
- ButureScript parser;
- geometry;
- DRC;
- ERC;
- Gerber;
- router;
- project migrations.

Arquivos inválidos nunca devem causar corrupção.

Implemente crash recovery.

Simule encerramento durante Save.

Verifique atomic writes.

---

# PROMPT 46 — UX profissional

Faça auditoria completa de UX.

ButureCAD é ferramenta de engenharia.

Priorize velocidade operacional.

Revise:

- keyboard shortcuts;
- context menus;
- selection;
- multi-selection;
- drag;
- copy/paste;
- duplicate;
- align;
- distribute;
- properties;
- tool switching;
- layer switching;
- routing controls;
- zoom/pan.

Adicione:

- repeat last command;
- recent tools;
- command palette.

Usuários experientes devem conseguir trabalhar principalmente pelo teclado quando desejarem.

---

# PROMPT 47 — Projeto real de validação

Agora NÃO adicione novas features.

Vamos validar ButureCAD fazendo uma PCB real de teste.

Crie um projeto de referência contendo:

- microcontroller;
- USB;
- 3.3V regulator;
- crystal;
- decoupling capacitors;
- LEDs;
- buttons;
- programming/debug header.

Faça:

- schematic;
- ERC;
- footprint assignment;
- PCB;
- placement;
- routing;
- vias;
- ground plane;
- DRC;
- 3D preview;
- Gerber;
- drill;
- BOM;
- Pick and Place.

Documente todos os problemas encontrados durante esse fluxo.

Classifique:

- BLOCKER;
- CRITICAL;
- MAJOR;
- MINOR.

Corrija BLOCKER e CRITICAL.

---

# PROMPT 48 — Auditoria de funcionalidades

Faça uma auditoria completa do ButureCAD comparando nossas capacidades com o fluxo esperado de um EDA moderno.

Não adicione features durante a primeira etapa.

Crie uma matriz:

Feature
Implemented
Partial
Missing
Broken
Test Coverage
Priority

Analise:

- schematic;
- hierarchical schematic;
- symbols;
- ERC;
- PCB;
- footprints;
- routing;
- vias;
- multilayer;
- DRC;
- net classes;
- differential pairs;
- length matching;
- zones;
- stackup;
- 3D;
- manufacturing;
- Gerber viewer;
- libraries;
- simulation;
- plugins;
- ButureScript;
- CLI;
- Agent API.

Não diga que algo funciona apenas porque existe código.

Teste realmente cada feature.

Crie:

docs/FEATURE_AUDIT.md

---

# PROMPT 49 — Limpeza para Release

Prepare ButureCAD para primeira Alpha pública.

Procure:

- TODO;
- FIXME;
- mock;
- placeholder;
- fake data;
- dead code;
- debug code;
- unused dependencies;
- panic!;
- unwrap() perigoso;
- memory leaks;
- race conditions.

Revise mensagens de erro.

Verifique:

- Windows;
- Linux;
- macOS.

Não adicione features novas.

Corrija estabilidade e consistência.

---

# PROMPT 50 — Auditoria final

Atue como:

- Senior EDA Engineer;
- Senior Rust Engineer;
- Senior PCB Designer;
- Senior Desktop Software Architect.

Faça uma auditoria final rigorosa do ButureCAD.

Dê notas de 0 a 10 para:

- Architecture;
- Performance;
- Schematic;
- PCB;
- Routing;
- ERC;
- DRC;
- Libraries;
- 3D;
- Manufacturing;
- Simulation;
- Plugins;
- ButureScript;
- Agent API;
- UX;
- Reliability.

Compare conceitualmente nosso fluxo com ferramentas EDA profissionais.

NÃO falsifique equivalência.

Liste claramente:

1. O que está production-ready.
2. O que funciona mas ainda é experimental.
3. O que está parcialmente implementado.
4. O que está faltando.
5. O que impediria um engenheiro de usar ButureCAD para uma PCB comercial real.

Crie:

docs/PRODUCTION_READINESS.md

Depois gere roadmap priorizado:

P0
P1
P2
P3

Não implemente novas features até a auditoria terminar.

---

# Arquitetura final esperada

```text
                           BUTURECAD
                               │
              ┌────────────────┴────────────────┐
              │                                 │
        Desktop GUI                       buturecad-cli
   Tauri + React + TS                          │
              │                                │
              └──────────────┬─────────────────┘
                             │
                    ┌────────▼────────┐
                    │ ButureCAD Core  │
                    │      Rust       │
                    └────────┬────────┘
                             │
        ┌──────────┬─────────┼─────────┬────────────┐
        │          │         │         │            │
        ▼          ▼         ▼         ▼            ▼
   Schematic      PCB      Router    ERC/DRC   Manufacturing
        │          │         │         │            │
        └──────────┴─────────┼─────────┴────────────┘
                             │
                         EDA IR
                             │
              ┌──────────────┼──────────────┐
              │              │              │
              ▼              ▼              ▼
        ButureScript      Plugins       Agent API
                                             │
                                             ▼
                                        AI Agents
```

# Fluxo tradicional esperado

```text
Create Project
      ↓
Schematic
      ↓
ERC
      ↓
Assign Footprints
      ↓
Update PCB
      ↓
Board Outline
      ↓
Placement
      ↓
Ratsnest
      ↓
Route Tracks
      ↓
Vias / Layers
      ↓
Copper Zones
      ↓
DRC
      ↓
3D Viewer
      ↓
Manufacturing Preview
      ↓
Gerber + Drill + BOM + Pick&Place
```

# Fluxo Code-First esperado

```text
ButureScript
      ↓
Parser
      ↓
AST
      ↓
Semantic Validation
      ↓
EDA IR
      ↓
ButureCAD Core
      ↓
Schematic / PCB
      ↓
ERC / DRC
      ↓
Manufacturing
```

# Fluxo com IA esperado

```text
User Prompt
     ↓
AI Agent
     ↓
ButureCAD Agent API
     ↓
ButureScript / Structured Commands
     ↓
Validation
     ↓
ERC / DRC
     ↓
Visual Diff
     ↓
User Approval
     ↓
Command Engine
     ↓
Project
```

# Princípio fundamental

O ButureCAD NÃO deve depender de IA.

O ButureCAD deve ser um EDA completamente funcional por conta própria.

IA será apenas outra forma de controlar um sistema que já possui:

- GUI;
- CLI;
- ButureScript;
- Plugin API;
- Agent API.

Isso significa que humanos, scripts, plugins e agentes utilizam o mesmo núcleo confiável.

# Meta do projeto

O objetivo não é criar uma interface que pareça um EDA.

O objetivo é construir progressivamente um EDA funcional.

Nenhuma feature deve ser considerada implementada simplesmente porque existe um botão correspondente.

PCB routing precisa realmente criar geometria.

ERC precisa realmente analisar conectividade.

DRC precisa realmente analisar geometria e regras.

Gerber precisa realmente representar a placa.

ButureScript precisa realmente compilar para o modelo interno.

Agent API precisa realmente operar sobre o mesmo Core.

Sempre priorize funcionamento real sobre aparência de completude.