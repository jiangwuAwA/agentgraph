# 浜у搧鏀瑰杽浠诲姟鍒楄〃锛堟簮鑷?PM 澶嶅 路 v0.5.1锛?
> **鐘舵€佸浘渚嬶細** open / in_progress / blocked / done  
> **鍘熷垯锛?* 鍏堣瘉鎹笌鎺ラ€氾紝鍐嶅姞鍒嗘瀽鐗规€э紱涓嶈秴鍞?sound锛涘叏灞€ `--with-macro` 浠嶉粯璁?OFF銆? 
> **鍏宠仈锛?* [product-boundary-migration.md](product-boundary-migration.md)銆乕agent-recipes.md](agent-recipes.md)銆乕noise-governance.md](noise-governance.md)銆乕workspace.md](workspace.md)

---

## P0 鈥?鎶婅兘鍔涘彉鎴愩€屽彲琚€夌敤鐨勪骇鍝併€嶏紙浼樺厛锛?
### P0-1 鍏紑 Agent 鏀圭爜浠诲姟璇勬祴

| 瀛楁 | 鍐呭 |
|---|---|
| **鐩爣** | 鐢ㄥ彲澶嶇幇浠诲姟璇佹槑锛氭寕涓?agentgraph 鍚庯紝Agent 鐖嗙偢鍗婂緞鏇村噯銆佽鏀规枃浠舵洿灏?|
| **浜や粯鐗?* | `evals/agent-tasks/`锛氫换鍔￠泦锛坕ssue + 浠?fixture + 鏈熸湜缁撴瀯浜嬪疄锛夛紱璇勫垎鍗忚锛堣竟/鏂囦欢鍛戒腑銆佽鏀癸級锛沨arness锛圕LI 鎴?MCP 璋冪敤锛夛紱`docs/eval-agent-tasks.md` 鏁板瓧琛?|
| **鑼冨洿** | 鈮? 涓换鍔★細骞插噣 TS/Py/Go 鍚?1鈥?锛汻ust monorepo workspace 2锛泆nsafe/sound-disabled 1锛堥獙 scoped 寮曞锛夛紱鍣０鍚?`fmt` 绫?1 |
| **闈炵洰鏍?* | 涓嶆帴绉佹湁 stock 婧愮爜锛涗笉瀹ｇО鐢熶骇 monorepo 绮惧害 |
| **楠屾敹** | `cargo test` 鎴栬剼鏈彲璺戦€?harness锛汻EADME/recipes 寮曠敤鏁板瓧锛涚浉瀵广€屾棤宸ュ叿 / 浠?LLM銆嶅熀绾胯〃 |
| **娑夊強** | `evals/` 鎴?`fixtures/eval-agent-tasks/`銆佹柊 docs銆丆I 鍙€?job |
| **浼?* | 1鈥? 鍛?|
| **浜や粯 (this slice)** | **done (public first ship):** `fixtures/eval-agent-tasks/`锛? 涓叕寮€浠诲姟锛? `scripts/eval_agent_tasks.py` + `tests/agent_task_eval.rs` + [docs/eval-agent-tasks.md](eval-agent-tasks.md)銆傚熀绾夸负 **name-grep**锛堝疄鐜颁簬 harness锛岄潪浼€?LLM 鏁板瓧锛夛紱绂佹绉佹湁 corpus 璺緞銆?|

### P0-2 鎺ラ€氬寘 Onboarding Kit锛? 鍒嗛挓 MCP锛?
| 瀛楁 | 鍐呭 |
|---|---|
| **鐘舵€?* | **done** |
| **鐩爣** | 鏂扮敤鎴?Agent 瀹夸富 5 鍒嗛挓鍐咃細瑁呭ソ 鈫?绱㈠紩绀轰緥浠?鈫?璋冪敤 `blast_radius` |
| **浜や粯鐗?* | `docs/onboarding.md`锛沗examples/mcp-claude.json` / `examples/mcp-generic.json`锛沗scripts/demo_blast_radius.ps1` + `scripts/demo_blast_radius.sh`锛汻EADME / agent-recipes / playbook 閾炬帴 |
| **鑼冨洿** | 涓€鏉?happy path + 涓€鏉?workspace happy path |
| **闈炵洰鏍?* | 涓嶆浛浠ｅ畬鏁?README锛涗笉缁戞鏌愪竴 Agent 浜у搧 |
| **楠屾敹** | 鎸夋枃妗ｅ喎鍚姩鍙垚鍔燂紱docs_claims 涓嶇孩锛涙紨绀鸿剼鏈?exit 0 |
| **娑夊強** | `docs/`銆乣examples/`銆乣scripts/`銆丷EADME 椤堕儴閾炬帴 |
| **浼?* | 3鈥? 澶?|

### P0-3 涓昏矾寰勪竴椤电焊锛圧EADME 鏀舵潫锛?
| 瀛楁 | 鍐呭 |
|---|---|
| **鐩爣** | 浜у搧涓昏矾寰勪竴鐪煎彲瑙侊細3 涓?MCP tool + 浣曟椂 sound/涓嶄細 |
| **浜や粯鐗?* | README en/zh 椤堕儴 **Agent path** 鍖猴細`blast_radius` / `who_calls` / `graph`锛? `index`锛夛紱璇氬疄琛紙window銆乻ubset_ok銆乶ote锛夛紱Advanced 閾惧埌 flags 鍏ㄦ枃 |
| **鑼冨洿** | 鍙欎簨涓庣粨鏋勶紝涓嶅垹鑳藉姏鏂囨。 |
| **闈炵洰鏍?* | 涓嶅垹闄ゅ簳灞?CLI/MCP 琛?|
| **楠屾敹** | 鏂颁汉 30 绉掑唴鐭ラ亾銆孉gent 璇ヨ皟浠€涔堛€嶏紱鏃犺秴鍞彞锛沝ocs_claims 缁?|
| **娑夊強** | `README.md` / `README.zh-CN.md`銆乣docs/agent-recipes.md` 閾炬帴 |
| **浼?* | 1鈥? 澶?|

### P0-4 `blast_radius` recommendation 寮哄寲锛坰coped sound 寮曞锛?
| 瀛楁 | 鍐呭 |
|---|---|
| **鐩爣** | S 鍏抽椄/scope 鏃讹紝榛樿杈撳嚭鐩存帴鍛婅瘔 Agent **涓嬩竴姝ュ悎娉曞懡浠?* |
| **浜や粯鐗?* | `window=disabled/default` 鏃?`recommendation` 鍚細`sound_candidates[]`銆佺ず渚?`impact --sound --workspace-root <id>`銆乣by_top_dir` 鎻愮ず锛涜剰 union **姘镐笉** `window=sound`锛堝凡鏈夋祴璇曚繚鎸侊級 |
| **鑼冨洿** | `query/recipes` + CLI/MCP payload + 鍗曟祴 |
| **闈炵洰鏍?* | 涓嶈嚜鍔ㄦ墽琛?scoped sound锛涗笉闈欓粯 union 瀹忚竟 |
| **楠屾敹** | `tests/agent_recipes.rs` / `r32` 鎵╁睍锛沺ayload 閿ǔ瀹氾紙鍙繘 agent-recipes銆屽嬁鏀瑰悕銆嶅垪琛級 |
| **娑夊強** | `src/query/recipes`銆乣src/cli.rs`銆乣src/mcp/server.rs`銆乣docs/agent-recipes.md` |
| **浼?* | 2鈥? 澶?|

### P0-5 鐪熷疄 Agent 瀵圭収璇勬祴锛圓gent卤MCP锛?
| 瀛楁 | 鍐呭 |
|---|---|
| **鐘舵€?* | **done (scripted S1+S2 + live P0-5b host-session slice):** 鍗忚 + 鍥炴斁 + 鑴氭湰鍖?policy A/B/C + live host-session A/B銆係cripted = **闈?* standardized lab LLM锛沴ive = 鍗?host session锛坄mimo-desktop-host-session`锛夛紝**闈?* public benchmark model锛?*鏃犺秴鍞?*锛坙ive A/B 鍣０鏈垎绂伙級 |
| **鐩爣** | 鍦ㄥ叕寮€浠诲姟闆嗕笂鍥炵瓟浜у搧闂锛?*鎸備笂 agentgraph MCP 鐨?Agent锛屾槸鍚︽瘮涓嶆寕鏇村噯銆佽鏀规洿灏?* 鈥?涓嶅啀浠呯敤 name-grep 浠ｆ浛 Agent 琛屼负 |
| **鑳屾櫙** | P0-1 宸蹭氦浠?structure-fact 鎵撳垎 vs **name-grep**锛堢‘瀹氭€?token 鍩虹嚎锛夈€傛枃妗ｅ凡璇氬疄澹版槑杩欎笉鏄?LLM/Agent 浜у搧璇佹槑銆侾0-5 琛ラ綈璇ュ鐓?|
| **浜や粯鐗?* | 鈶?`evals/agent-baseline/`锛堟垨 `fixtures/eval-agent-tasks/` 鎵╁睍锛夛細鍚屼竴濂楀叕寮€ mini-repo + 浠诲姟 JSON锛涒憽 harness 鍗忚锛?*A 缁?* Agent 浠呭厑璁?grep/read锛?*B 缁?* Agent 鍏佽 MCP `blast_radius`/`who_calls`/`graph`锛堟垨 CLI recipes锛夛紱鈶?璇勫垎锛氭湡鏈涙枃浠?绗﹀彿鍛戒腑銆?*鍣０鏂囦欢**銆侀敊璇敼鐮佽寖鍥达紙鑻ユ湁琛ヤ竵锛夛紱鈶?`docs/eval-agent-baseline.md`锛氫换鍔＄骇涓庢眹鎬昏〃銆佸鐜板懡浠ゃ€佹ā鍨?娓╁害/閲嶅娆℃暟锛涒懁 鍙€夛細鎶婃眹鎬绘寚鏍囨寕鍒?README / eval-agent-tasks 浜ゅ弶閾炬帴 |
| **鑼冨洿** | 浼樺厛澶嶇敤 P0-1 鐨?9 涓叕寮€浠诲姟锛堣嚦灏戣窇閫?**鈮?** 涓級锛涘浐瀹?prompt 妯℃澘涓庛€岀姝㈣秴鍞€嶆寚浠わ紱姣忎换鍔?鈮 娆￠噸澶嶏紙寤鸿 N鈮?锛夋姤 mean锛涜褰曞け璐ユā寮?|
| **鍩虹嚎璇存槑** | **B鈭扐** 涓轰富瑕佷骇鍝佹寚鏍囷紱name-grep 淇濈暀涓?**纭畾鎬т笅鐣?瀵圭収**锛屼笉鍒?P0-1 琛?|
| **闈炵洰鏍?* | 涓嶄吉閫犳湭璺戦€氱殑 LLM 鏁板瓧锛涗笉鎻愪氦绉佹湁 stock锛涗笉鎶婂崟娆″垢杩?run 鍐欐垚缁撹锛涗笉鎶?`window=sound` 鍐欐垚鐢熸€?sound |
| **绾︽潫** | Agent 杩愯鐜闇€缃戠粶/API 鐨勯儴鍒嗭細**鍙€?CI / 鎿嶄綔鍛樿建** 鈥?涓讳粨 harness 蹇呴』鑳藉湪 **鏃?LLM** 鏃朵粛瑙ｆ瀽銆屽綍鍒跺ソ鐨?tool 杞ㄨ抗銆嶅仛鍥炴斁鎵撳垎锛堣嫢鍏堜氦浠樺綍鍒跺崗璁級 |
| **楠屾敹** | 鈶?鏂囨。鍚?A/B 瀹氫箟涓庨潪澹扮О锛涒憽 鑷冲皯涓€浠藉彲澶嶇幇姹囨€昏〃锛堜换鍔?脳 A/B 鎸囨爣锛夛紱鈶?harness/鑴氭湰鍙湰鍦拌窇锛涒懀 涓?eval-agent-tasks 鐨勫叧绯诲啓娓咃紙structure facts vs Agent 琛屼负锛夛紱鈶?docs_claims / 鐩稿叧 `cargo test` 缁匡紱鈶?backlog 鏈崱鐘舵€佹敼涓?done |
| **娑夊強** | `docs/eval-agent-baseline.md`銆乣docs/eval-agent-tasks.md`锛堜氦鍙夐摼鎺ワ級銆乣scripts/eval_agent_baseline.py`锛堟垨绛変环锛夈€乣tests/agent_baseline.rs`锛堝彲閫夛細鍗忚/鍥炴斁閿侊級銆乣fixtures/eval-agent-tasks/**`锛堝彧璇诲鐢ㄤ紭鍏堬級銆丷EADME 閾炬帴涓€鍙?|
| **浼?* | 1鈥? 鍛紙鍚娆?Agent 璺戞壒锛涘洖鏀惧崗璁彲鍏堜簬 live LLM锛?|
| **寤鸿鍒囩墖** | **S1** 鍗忚 + 褰曞埗/鍥炴斁 JSON 鏍煎紡 + 鏃犵綉鎵撳垎锛?*S2** 鎿嶄綔鍛?live A/B 璺戞壒鍐欏叆 docs锛?*S3**锛堝彲閫夛級CI 鎵嬪姩 workflow_dispatch 浜у嚭 artifact |
| **浜や粯 (this slice)** | **done (scripted S1 + policy S2-sim + P0-5b live host-session):** [eval-agent-baseline.md](eval-agent-baseline.md) + `scripts/eval_agent_ab.py` + `evals/agent-ab/**`锛堚墺9 tasks 脳 鈮? A/B/C runs锛? `tests/agent_ab_eval.rs`锛?*P0-5b:** `scripts/eval_agent_ab_live.py` + `evals/agent-ab-live/**`锛? tasks 脳 3 seeds 脳 arms A/B = **54** trajectories锛? `tests/agent_ab_live.rs`銆?*鏍囩锛堝己鍒讹級锛?* scripted = **scripted tool-policy agents**锛沴ive = **host-session LLM**锛坄model_note=mimo-desktop-host-session`锛?*闈?* public benchmark model / **闈?* standardized lab harness锛夈€俹perator 鏍囩 **A=MCP/CLI recipes锛孊=read/grep锛孋=name-grep**銆係cripted numbers锛欰 noise **0.00**锛汢 **1.56**锛汣 **0.78**銆侺ive numbers锛坮ecalled only锛夛細A live recall 1.00 / noise **0.00**锛汢 live recall 1.00 / noise **0.00**锛堟湰 session 鏈垎绂?鈥?**绂佹**鎹瀹ｇО live MCP 浜у搧浼樺娍锛夛紱contamination + N small 宸插啓鍏?docs銆侽ffline replay锛歚python scripts/eval_agent_ab.py score --traj-dir evals/agent-ab` 涓?`--traj-dir evals/agent-ab-live`銆?*S3 CI workflow 鏈垏鐗囪烦杩囥€?* |
| **P0-5c 鐘舵€?* | **done (protocol + hard fixtures + 鈮? runners + extended metrics + recorded N=3):** 瑙佷笅鏉°€屼氦浠?(P0-5c)銆?|
| **P0-5d 鐘舵€?* | **done (S1 harness + S2 isolated live matrix):** `lab_ready=true` (mimo-pro + mimo-flash, N=5); **闈?*鐢熸€?sound |
| **浜や粯 (P0-5c)** | **done:** [eval-agent-baseline.md](eval-agent-baseline.md) 搂 **P0-5c** + `scripts/eval_agent_ab_c.py`锛坵rite/stamp/score/--runner锛? `fixtures/eval-agent-tasks-hard/**`锛堚墺4 hard tasks锛? `evals/agent-ab-c/**`锛? tasks 脳 2 runner kinds 脳 2 arms 脳 3 seeds = **48** trajectories + `task_randomization.json`锛? `tests/agent_ab_c_eval.rs` + `evals/agent-ab-c/README.md`銆?*Runner kinds:** `host_session_llm`锛坄independent_session=false`锛屾姭闇?contamination锛? `scripted_external_runner`锛坉ecision-path `independent_session=true`锛夈€?*Extended metrics:** `mcp_or_cli_calls` / `chose_correct_workspace_root` / `file_budget` / `read_budget` / `approx_tokens=null` / `runner_id` / `model_note` / `saw_labels_before_commit=false`銆?*Recorded hard-task means (no oversell):** host A noise **0.00** vs host B **1.50**锛泂cripted A noise **0.25** vs scripted B **3.25**锛坰cripted A recall **0.9375** 鈥?multi-root path alias gap 宸茶瘹瀹炶褰曪級銆?*N target 鈮?锛涙湰 host 璁板綍 N=3銆?* **绂佹**灏?host-session 鍒嗙鍐欐垚 multi-model lab / 浜у搧浼樿秺鎬ц瘉鏄庛€?*鏃?*鐙珛 isolated-subagent 绗笁 runner锛坕ncomplete锛屼笉浼€狅級銆?|

### P0-5d 鐪熼殧绂?live 瀵圭収锛坕solated lab锛?
| 瀛楁 | 鍐呭 |
|---|---|
| **鐘舵€?* | **done (S1 harness + S2 isolated live matrix):** `lab_ready=true`锛堣浜や粯锛?|
| **鐩爣** | 鍦?**鐪熼殧绂?* 鏉′欢涓嬪娴嬶細live Agent卤agentgraph 鏄惁鍦?hard 浠诲姟涓婄ǔ瀹氶檷浣庡櫔澹?/ 鎻愰珮 `workspace-root` 姝ｇ‘鐜?鈥?浣滀负鍙澶栧紩鐢ㄧ殑閫夌敤璇佹嵁鍊欓€?|
| **鑳屾櫙** | P0-5b easy fixture 涓?live A/B **鏈垎绂?*锛汸0-5c hard 涓婃湁鍣０涓?root 閫夋嫨淇″彿锛屼絾 `host_session_llm.independent_session=false`銆乫ixture 浣滆€呭悓 session銆丯=3銆乻cripted runner 闈?live LLM銆侾0-5d 琛ラ綈 lab 绾ч殧绂?|
| **浜や粯鐗?* | 鈶?鍗忚鎵╁睍锛歚docs/eval-agent-baseline.md` 搂 **P0-5d**锛堥殧绂汇€侀殢鏈哄寲銆佺洸璇勩€佺姝㈡薄鏌撴簮娓呭崟锛夛紱鈶?**Isolated runner 鎺ュ彛**锛氭瘡 arm/seed **鐙珛杩涚▼鎴栫嫭绔?agent 浼氳瘽**锛堟棤鍏变韩涓棿 file-set銆佹棤 fixture `task.json` expected 鍙锛夛紱鈶?**鈮? live runners**锛氳嚦灏?1 涓?**闈?host-session** 妯″瀷/runner id + 鍙€?host 浣滃鐓ц噦锛涒懀 浠诲姟闆嗭細澶嶇敤 `fixtures/eval-agent-tasks/**` + `eval-agent-tasks-hard/**`锛堝缓璁?easy鈮? + hard鈮?锛夛紱鈶?**N鈮? seeds / task / arm / runner**锛堟湭杈炬爣蹇呴』鍦ㄨ〃澶存爣绾級锛涒懃 杞ㄨ抗 `evals/agent-ab-d/**` + 闅忔満鍖栨棩蹇楋紱鈶?璇勫垎涓庢寚鏍囧榻?P0-5c锛坮ecall / extra-noise / cwr / mcp_calls / file_budget锛沗approx_tokens` 浠呯湡瀹炲€兼垨 null锛夛紱鈶?姹囨€昏〃 + 闈炲０绉?+ `docs_claims` 闂ㄧ锛涒懆锛堝彲閫夛級`tests/agent_ab_d_eval.rs` 閿佸崗璁瓧娈典笌銆岀姝吉閫?N/妯″瀷銆?|
| **鑼冨洿** | 涓绘寚鏍囦粛涓?structure-fact 鏂囦欢闆嗭紱浜у搧瑙ｈ缁村害锛?*noise 鍒嗙**銆?*cwr**銆?*鏄惁瑙﹀彂 scoped sound 寤鸿**銆傚厑璁告搷浣滃憳/澶栭儴 API runner锛涚粨鏋滃繀椤诲彲 `score` 鍥炴斁 |
| **闈炵洰鏍?* | 涓嶆妸 null 缁撴灉鍐欐垚闃虫€э紱涓嶅悎鎴愭湭璺戦€氱殑妯″瀷鏍硷紱涓嶆妸 scripted 鍐掑厖 live锛涗笉鎻愪氦绉佹湁 monorepo 婧愮爜锛涗笉鎶?`ast_modeled` 鍐欐垚鐢熸€?sound |
| **姹℃煋闂ㄧ锛堝繀椤伙級** | 鈶?鍐崇瓥璺緞鐪嬩笉鍒?golden/expected锛涒憽 arm 闂存棤鍏变韩鐘舵€佹枃浠讹紱鈶?浠诲姟椤哄簭闅忔満鍖栧苟钀界洏锛涒懀 runner 鍏冩暟鎹惈 `independent_session` / `model_note` / harness 鐗堟湰锛涒懁 鑻ヤ粛鐢?author-session锛?*鏁磋〃闄嶇骇涓?non-lab** 骞舵爣 `lab_ready=false` |
| **楠屾敹** | 鈶?鏂囨。鍐欐竻 lab vs non-lab 鍒ゅ畾锛涒憽 鈮? live runner + N鈮? 鐨勫畬鏁磋〃 **鎴?* 鏄庣‘ `lab_ready=false` + 缂哄彛娓呭崟锛涒憿 48+ 鍙洖鏀捐建杩癸紙鎸変换鍔℃暟脳鑷偯梥eed脳runner锛夛紱鈶?涓?P0-5b/c 宸紓琛紙easy vs hard銆乭ost vs isolated锛夛紱鈶?鐩稿叧娴嬭瘯/docs_claims 缁匡紱鈶?鏈崱鐘舵€佹敼涓?done 鎴?blocked锛堢己绗笁鏂?runner 鏃讹級 |
| **娑夊強** | `docs/eval-agent-baseline.md`銆乣scripts/eval_agent_ab_d.py`锛堟垨鎵╁睍 `eval_agent_ab_c.py --isolated`锛夈€乣evals/agent-ab-d/**`銆乣fixtures/eval-agent-tasks*/**`锛堝彧璇伙級銆丷EADME 涓€鍙ラ摼鎺ワ紙浠呭綋 lab_ready=true锛?|
| **浼?* | 1鈥? 鍛紙鍙栧喅浜庢槸鍚︽湁澶栭儴 live runner/API锛涙棤澶栭儴妯″瀷鍒欏厛浜や粯闅旂 harness + `lab_ready=false`锛?|
| **寤鸿鍒囩墖** | **S1** 闅旂鍗忚 + 澶栭儴 runner 鎺ュ彛 + 鐩茶瘎瀛楁锛?*S2** 绗簩 live runner 璺戞壒锛圢鈮?锛夛紱**S3** 姹囨€?+ 瀵瑰鍙欎簨锛堜粎 lab_ready=true 鏃跺彲绉?lab锛?|
| **浜や粯 (P0-5d)** | **done (S1 harness + S2 isolated live):** 鍗忚 + `scripts/eval_agent_ab_d.py` + `evals/agent-ab-d/**`锛坋asy 4 + hard 4锛沴ive runners `mimo-pro`/`mimo-flash` 脳 A/B 脳 N=5锛沗incomplete_cells=0`锛? `tests/agent_ab_d_eval.rs`銆?*lab_ready=true**锛坔arness 绂荤嚎鍒ゅ畾锛氣墺2 闈?author live銆丯鈮?銆佸弻鑷傘€乪asy+hard銆乮ndependent_session锛夈€?*姹囨€伙紙绂荤嚎 stamp锛夛細** live A recall **0.952鈥?.971** / noise **0.00**锛沴ive B recall **0.879鈥?.902** / noise **0.00**锛沜wr A **15/0** vs B **12/0**锛泂cripted isolated B noise **1.875**銆?*鍙欎簨绾緥锛?* live **鍣０鏈媺寮€**鈥斺€斿彧鍙紩鐢ㄥ彫鍥炰笌 cwr 淇″彿 + 鐭╅樀瀹屾暣鎬э紱**涓嶅緱**鍐欍€宭ive 鍣０浼樺娍銆嶆垨鐢熸€?sound銆俙approx_tokens=null`銆?|
| **渚濊禆** | P0-5c 杞ㄨ抗/鎸囨爣 schema 淇濇寔鍏煎锛沨ard fixtures 鍙鐢?|
| **骞惰鍙€?* | 淇?scripted A 鐨?multi-root **璺緞鍒悕/re-export 鍙洖缂哄彛**锛堜骇鍝佷晶锛岄潪鏈崱蹇呭仛锛?|

---

## P1 鈥?宸╁浐 monorepo 涓庝俊浠?
### P1-1 Workspace watch / CI 澧為噺

| 瀛楁 | 鍐呭 |
|---|---|
| **鐩爣** | 澶氭牴浠撴敼鏂囦欢鍚庝笉蹇呭叏閲?`index --workspace` 鎵嶈兘鏌?|
| **浜や粯鐗?* | `watch --workspace 鈥 鎴?`index_paths` 鎸?`root_id` 澧為噺锛涙枃妗ｄ笌 `workspace.md` 瀵归綈 |
| **楠屾敹** | 鍙?root 鏀逛竴渚?鈫?鍙︿竴渚?hash 涓嶉噸绠楋紱subset/diff 琛屼负鏈夋祴璇?|
| **浼?* | 1 鍛?|

### P1-2 Stale 鎻愮ず杩?MCP 榛樿 payload

| 瀛楁 | 鍐呭 |
|---|---|
| **鐩爣** | Agent 涓嶈 docs 涔熻兘鐪嬪埌 `baseline_stale` / `sidecar_stale` / workspace `missing` |
| **浜や粯鐗?* | 鏌ヨ/鐘舵€佺被 MCP 杩斿洖鍙€夋垨榛樿甯︿笂杩板瓧娈碉紙闈炵牬鍧忥細鏃у鎴风鍙拷鐣ワ級 |
| **楠屾敹** | e2e锛歸atch 鍚?`graph_diff`/`stats` 鍚?stale锛涙棤 sidecar 鏃?`sidecar_exists=false` |
| **浼?* | 2鈥? 澶?|

### P1-3 Golden Agent Suites锛堝彂鐗堥棬绂侊級

| 瀛楁 | 鍐呭 |
|---|---|
| **鐘舵€?* | **done** |
| **鐩爣** | 涓?docs_claims 鍚岀骇锛歳ecipe/window/璇氬疄瀛楁鍥炲綊涓嶉潬浜鸿 |
| **浜や粯鐗?* | `fixtures/eval-agent-goldens/` + `tests/agent_goldens.rs`锛涚撼鍏?CI `cargo test` |
| **楠屾敹** | 鏈熸湜 JSON 閿?`window`/`edge_role`/`recommendation` 鍏抽敭褰㈡€?|
| **浼?* | 3鈥? 澶?|
| **浜や粯 (this slice)** | **done:** `fixtures/eval-agent-goldens/`锛坧ublic fixtures + `goldens.json`锛? `tests/agent_goldens.rs` + [docs/agent-goldens.md](agent-goldens.md)銆俽ecommendation 鐢?contains/regex锛岀ǔ瀹氶敭鍕挎敼鍚嶃€?|

### P1-4 澶?workspace 绱㈠紩鎬ц兘涓庨绠楁枃妗?
| 瀛楁 | 鍐呭 |
|---|---|
| **鐘舵€?* | **done** |
| **鐩爣** | 鍙鏈燂細N 鏂囦欢 脳 M root 鐨?index/status 閲忕骇涓?soft SLO |
| **浜や粯鐗?* | `docs/eval-query-p95.md` workspace 鎵╁厖锛涘彲閫?`tests/query_p95` smoke锛堣蒋闂ㄧ锛?|
| **楠屾敹** | 鏂囨。鍚鐜板懡浠わ紱涓?operator 瀹炴祴涓€鑷村鏍囨槑 |
| **浼?* | 1鈥? 澶?|
| **浜や粯 (this slice)** | **done:** [eval-query-p95.md](eval-query-p95.md) workspace 鎵╁厖锛堟満鍣?鏁板瓧/澶嶇幇/闈?SLO 璇氬疄锛? `tests/perf_workspace.rs` 杞?smoke銆?|

---

## P2 鈥?澧為暱涓庡樊寮傚寲锛堜笉鎸?P0锛?
| ID | 浠诲姟 | 璇存槑 | 浼?| 鐘舵€?|
|---|---|---|---|---|
| **P2-1** | 浠撳簱绾?macro 榛樿閰嶇疆 | 椤圭洰/env 鎵撳紑銆屾湁 fresh sidecar 鍒?blast_radius 鍙?include銆嶏紱**鍏ㄥ眬浠?OFF** | 2鈥? 澶?|
| | **鐘舵€?* | **done:** `.agentgraph/config.toml` / `agentgraph.toml` `macro_default=off\|if_fresh\|on`; env `AGENTGRAPH_MACRO_DEFAULT` overrides file; CLI explicit wins. `if_fresh` + fresh sidecar 鈫?`include_macro=true` + `include_macro_reason=repo_config_if_fresh`. Sound/stale/nested/missing still refuse. `macro status` shows `macro_default_source`. Global default remains **OFF**. Tests: `tests/macro_default_config.rs`. |
| **P2-2** | CI 鐖嗙偢鍗婂緞娉ㄩ噴 demo | GitHub Action 鏍蜂緥锛歅R 瑙﹀彂 `blast_radius` 鈫?璇勮锛涗笉杩?required 涔熷彲 | 2鈥? 澶?| **done (demo slice):** `.github/workflows/blast-radius-demo.yml` + `examples/ci/blast-radius.yml` + `scripts/ci_blast_radius_markdown.py` + [ci-blast-radius-demo.md](ci-blast-radius-demo.md)銆係tep summary + soft-fail PR comment锛涙棤 expand install锛涢潪 required銆?|
| **P2-3** | 鏇村 L1 瑙勫垯锛堟湁 eval 鎵嶅仛锛?| 浠呭綋 golden 鎻愬崌 + 鍣０鍙帶 | 鎸夊寘 | open锛坋val-gated锛?|
| **P2-4** | 瀵瑰涓€鍙ヨ瘽绔炰簤鍙欎簨 | README 瀵规瘮琛ㄦ敹鏉燂細vs RAG / CodeQL / LSP / bare SCIP | 1 澶?| **done:** README en/zh銆孭ositioning / 瀹氫綅銆嶈〃锛圕hunk RAG / CodeQL enterprise / bare LSP / raw SCIP锛? 璇氬疄闈炲０绉帮紱閾炬帴 [eval-agent-tasks.md](eval-agent-tasks.md) + [onboarding.md](onboarding.md)锛沝ocs_claims 缁裤€?|

### 鏄庣‘涓嶅仛锛堣繎涓や釜瀛ｅ害锛?
- 鐢熸€?sound 钀ラ攢  
- 閫氱敤浠ｇ爜鎼滅储閲嶅仛  
- 鍏ㄥ眬 `--with-macro` / 鐩茬洰 `--recall` 榛樿  

---

## 2026-09-18 澶嶆牳缁撹锛堜簩娆★級

| ID | 澶嶆牳 |
|---|---|
| P0-1鈥0-4 | **shipped** 鈥?浠ｇ爜/鏂囨。/娴嬭瘯榻愶紱`agent_task_eval`/`agent_goldens`/`agent_recipes`/`docs_claims`/`e2e_cli`/`noise_roles` 鍏ㄧ豢 |
| P0-5 | **shipped (scripted + 5b host-session + 5c multi-runner hard + 5d isolated lab matrix)** 鈥?harness + 杞ㄨ抗榻愶紱**P0-5d `lab_ready=true`**锛坢imo-pro/flash 脳 N=5锛夛紱live 鍣０鏈垎绂伙紝涓讳俊鍙蜂负 **recall + cwr**锛?*鏃犺秴鍞?* |
| P1-1鈥1-4 | **shipped** 鈥?workspace watch銆丮CP stale 瀛楁銆乬oldens銆乸erf_workspace 鏂囨。+smoke |
| P2-1, P2-2, P2-4 | **shipped** 鈥?macro_default锛堝叏灞€ OFF锛夈€丆I demo锛堥潪 required锛夈€丷EADME 瀹氫綅 |
| P2-3 | **open锛坋val-gated锛?* 鈥?绗﹀悎銆屾棤 eval 鏁板瓧涓嶅仛銆?|

**娈嬪樊锛堜綆浼樺厛锛夛細**

1. P0-1 鍩虹嚎鏄?**name-grep**锛屼笉鏄湡瀹?LLM/Agent 鍩虹嚎 鈥?鏂囨。宸茶瘹瀹炲０鏄庛€傗啋 P0-5/5b/5c/**5d** 宸蹭氦浠橈紱**P0-5d `lab_ready=true`**锛堣 [eval-agent-baseline.md](eval-agent-baseline.md)锛夈€俁esidual锛堥潪闃诲锛夛細绗笁鏂?API runner銆佹洿澶?N銆佺湡瀹炶剰 monorepo 涓婄殑 live 鍣０鍒嗙銆乣approx_tokens` 鐪熷疄璁￠噺銆?2. ~~`fixtures/**/.agentgraph/index.db` 浜岃繘鍒剁储寮晘~ 鈥?**done**锛氭牴 `.gitignore` 澧炲姞 `**/.agentgraph/`锛涙湰鍦?fixture 绱㈠紩鐩綍宸插垹闄わ紙鍕垮啀鎻愪氦锛夈€?3. Session 浠诲姟闈㈡澘 ID锛圱7鈥揟18锛変笌鏂囨。 P0-x 缂栧彿涓嶄竴鑷?鈥?**浠ユ湰鏂囨。涓哄噯**銆?4. Workspace **union callers** CLI 鍚?spawn 鏃?p95 鍙埌绉掔骇锛堟枃妗ｅ凡鏍囬潪 SLO锛夆€?Agent 渚?*浼樺厛 root filter 鎴?MCP**锛涘凡鍐欏叆 eval-agent-tasks / recipes銆?5. **鐪熷疄 monorepo package map eval锛堥潪闃诲 residual锛?* 鈥?next-cut A 宸蹭氦浠?workspace **partial package alias**锛圕LI `--workspace-alias` / tsconfig paths / package.json `name`锛涜 [workspace.md](workspace.md)锛夈€侶ard fixture `ts-multi-root-client` 宸插甫 `packages/*/package.json` + e2e 閿侊紙`hard_fixture_package_json_enables_cross_root_link`锛夈€備粛鍦?open 鐨?residual锛氬湪**鐪熷疄鑴?monorepo**涓婅瘎娴嬪寘鍥捐鐩栫巼锛坋xports conditions銆乣node_modules`銆佸祵濂?workspace 鍖呫€侀噸鍚嶅寘锛夛紝浠ュ強 post-fix 璇勬祴杞ㄨ抗 optional rerun銆?*绂佹**鎶?partial package map 鍐欐垚 full TS resolution / 鐢熸€?sound銆?
---

## 寤鸿鎵ц椤哄簭

```text
锛堝凡 shipped锛塒0-1鈥0-4, P0-5/5b/5c/5d, P1-1鈥1-4, P2-1/2/4
  鈫?next-cut A cross-root package alias 鈥?**shipped**锛坧artial package map锛涜 workspace.md锛?  鈫?residual锛堥潪闃诲锛夛細鐪熷疄 monorepo package map eval锛沴ive 鍣０鍒嗙 / 鏇村ぇ N
  鈫?P2-3 L1 瑙勫垯锛坋val-gated锛?```

## 涓?session 浠诲姟闈㈡澘鐨勬槧灏?
- **鏉冨▉缂栧彿涓庨獙鏀舵爣鍑嗭細浠ユ湰鏂囨。 P0-x / P1-x / P2-x 涓哄噯銆?*
- Session 闈㈡澘 ID锛堝 T7鈥揟18锛変粎浣滀細璇濊窡韪紝**涓庢枃妗ｇ紪鍙蜂笉涓€鑷存椂蹇界暐闈㈡澘 ID**銆?- 闈㈡澘鎽樿搴斿敖閲忓啓鍏ユ枃妗ｇ紪鍙凤紙渚嬶細`P0-5 鈥锛夛紝渚夸簬瀵圭収銆?
---

*缁存姢锛氫骇鍝?浠撳簱璐熻矗浜恒€傚彉鏇存椂鍚屾 agent-recipes 绋冲畾閿笌 docs_claims銆?
