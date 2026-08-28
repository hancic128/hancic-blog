/**
 * Hancic 文章编辑器封装（milkdown v7 WYSIWYG）。
 * 打包为 IIFE，暴露 window.HancicEditor.create(opts)，供 admin.js（经典脚本）使用。
 * 构建（产物 milkdown.min.js 已提交入库）：
 *   cd /tmp/milkdown-build && npm i @milkdown/core@7.22.0 @milkdown/preset-commonmark@7.22.0 \
 *     @milkdown/plugin-history@7.22.0 @milkdown/plugin-clipboard@7.22.0 \
 *     @milkdown/plugin-listener@7.22.0 esbuild
 *   cp 本文件为 src.js 后：npx esbuild src.js --bundle --minify --format=iife \
 *     --outfile=milkdown.js && cp milkdown.js <repo>/assets/vendor/milkdown/milkdown.min.js
 * 说明：editor.action 内用 ProseMirror schema 节点（editorViewCtx）插入内容，
 * 不用 @milkdown/utils 的 insert（v7 需额外 context 注入，会报 Context not found）。
 * opts: {
 *   el: HTMLElement,            // 编辑器挂载容器
 *   content: string,            // 初始 Markdown
 *   isNewPost: boolean,         // 新建页：启用 localStorage 本地草稿
 *   draftKey: string,           // 本地草稿 key
 *   onUpdate: (md) => void,     // 文档变化回调（自动保存/本地草稿）
 *   onUpload: (files) => Promise<[{path, orig_name}]>  // 图片上传回调
 * }
 * 返回：{ getMarkdown, insertImage, insertVideo, insertLink, setContent, destroy }
 */
import { Editor, rootCtx, defaultValueCtx, editorViewCtx, parserCtx, serializerCtx } from "@milkdown/core";
import { commonmark } from "@milkdown/preset-commonmark";
import { gfm } from "@milkdown/preset-gfm";
import { history } from "@milkdown/plugin-history";
import { clipboard } from "@milkdown/plugin-clipboard";
import { listener, listenerCtx } from "@milkdown/plugin-listener";
import { prism, prismConfig } from "@milkdown/plugin-prism";
import python from "refractor/python";
import yaml from "refractor/yaml";
import sql from "refractor/sql";
import bash from "refractor/bash";
import json from "refractor/json";
import rust from "refractor/rust";
import toml from "refractor/toml";
import ini from "refractor/ini";
import { $prose, $useKeymap } from "@milkdown/utils";
import { Plugin, PluginKey, TextSelection } from "@milkdown/prose/state";
import { setBlockType, toggleMark, wrapIn, lift } from "@milkdown/prose/commands";
import { Fragment } from "@milkdown/prose/model";
import { Decoration, DecorationSet } from "@milkdown/prose/view";

// ---- Typora 式源码提示：光标所在段落高亮 + 段首/行内浅色显示未渲染的 markdown 标记 ----
// widget 不占文档位置（ProseMirror 限制），光标无法真正进入标记内部；
// 近似实现：点击标记把光标定位到对应内容边界；光标位于块首时标记显示激活态。
var activeEditor = null;
function setCursorTo(pos) {
  if (!activeEditor) return;
  activeEditor.action(function (ctx) {
    var view = ctx.get(editorViewCtx);
    var resolved = view.state.doc.resolve(Math.max(0, Math.min(pos, view.state.doc.content.size)));
    view.dispatch(view.state.tr.setSelection(TextSelection.near(resolved)));
    view.focus();
  });
}
function makeMdWidget(text, targetPos, active) {
  return function () {
    var span = document.createElement("span");
    span.className = "hancic-md-symbol" + (active ? " hancic-md-symbol-active" : "");
    span.textContent = text;
    // 点击标记：光标定位到对应内容边界（模拟源码中在标记处放置光标）
    if (targetPos !== undefined && targetPos !== null) {
      span.addEventListener("mousedown", function (e) {
        e.preventDefault();
        e.stopPropagation();
        setCursorTo(targetPos);
      });
    }
    return span;
  };
}

// 光标所在 text node 的 mark 标记（strong/em/inline_code/link）
function textNodeHint(blockNode, cursorOffset) {
  var acc = 0;
  var found = null;
  blockNode.forEach(function (child, off) {
    if (found) return;
    if (child.isText && cursorOffset >= acc && cursorOffset <= acc + child.text.length) {
      found = { pos: off, len: child.text.length, marks: child.marks || [] };
    }
    acc += child.nodeSize;
  });
  if (!found) return null;
  var before = "";
  var after = "";
  found.marks.forEach(function (m) {
    if (m.type.name === "strong") { before += "**"; after = "**" + after; }
    else if (m.type.name === "em") { before += "*"; after = "*" + after; }
    else if (m.type.name === "inline_code") { before += "\`"; after = "\`" + after; }
    else if (m.type.name === "link") {
      before += "[";
      after = "](" + (m.attrs.href || "") + ")" + after;
    }
  });
  if (!before && !after) return null;
  return { pos: found.pos, len: found.len, before: before, after: after };
}

// 块级标记：标题 #、引用 >、代码块 ```、列表 - / 1. / todo
function blockMarker(node) {
  if (node.type.name === "heading") {
    return "#".repeat(node.attrs.level || 1) + " ";
  }
  if (node.type.name === "blockquote") return "> ";
  if (node.type.name === "code_block") {
    var lang = (node.attrs.language || node.attrs.lang || "").trim();
    return "\`\`\`" + (lang ? lang : "") + "\n";
  }
  if (node.type.name === "list_item") {
    if (node.attrs.todo !== undefined) {
      return node.attrs.todo ? "- [x] " : "- [ ] ";
    }
    var list = null;
    return "- "; // 具体编号在插件里按父列表计算
  }
  return "";
}

// ---- 代码块语法高亮（refractor）：补充常见语言，支持 ```json/yaml/sql/python/bash 等指定 ----
function configurePrism(ctx) {
  ctx.set(prismConfig.key, {
    configureRefractor: function (r) {
      [python, yaml, sql, bash, json, rust, toml, ini].forEach(function (lang) { r.register(lang); });
      // env 是 ini 的常见别名（.env / dotenv 配置），映射到 ini 语法高亮
      try { r.alias("ini", "env"); } catch (e) { /* 低版本 refractor 无 alias，忽略 */ }
    }
  });
}

// 代码块退出：光标在代码块内按 Ctrl/Cmd+Enter → 在块后插入空段落并移出
// （代码块内 Enter 只换行，需显式退出键，否则光标困在块内无法移出/删除）
var exitCodeBlock = $useKeymap("HancicExitCodeBlock", {
  "Mod-Enter": {
    shortcuts: "Mod-Enter",
    command: function () {
      return function (state, dispatch) {
        var $from = state.selection.$from;
        if ($from.parent.type.name !== "code_block") return false;
        var pos = $from.after();
        var tr = state.tr.insert(pos, state.schema.nodes.paragraph.create());
        tr.setSelection(TextSelection.near(tr.doc.resolve(pos + 1)));
        if (dispatch) dispatch(tr);
        return true;
      };
    }
  }
});

// 块级标记的键盘删除（Typora 式）：光标位于块**内容起点**按 Backspace。
// heading 的 Backspace 由 commonmark 自带 keymap 处理（downgradeHeading 逐级降级
// 到段落）；列表项/引用/代码块这里显式处理（milkdown 的 liftFirstListItem 依赖
// joinBackward，列表前存在其它块时会 join 失败而毫无反应）：
//   列表项 → lift 出列表（脱离列表变独立段落）
//   引用   → lift 出 blockquote（整体解除，不再嵌套）
//   代码块 → 转为正文段落
// 注册顺序在 commonmark 之后：milkdown 的 keymap 倒序查找，本 keymap 的
// Backspace 优先执行，非上述场景返回 false 放行给 commonmark 处理。
var delBlockMarker = $useKeymap("HancicDelBlockMarker", {
  Backspace: {
    shortcuts: "Backspace",
    // priority 高于 baseKeymap（默认 50）与 commonmark，确保块首退格先处理标记
    priority: 60,
    command: function () {
      return function (state, dispatch) {
        var sel = state.selection;
        if (!sel.empty || sel.$from.parentOffset !== 0) return false;
        var $from = sel.$from;
        // 引用：光标块在 blockquote 内 → 解除引用（lift 会把光标块提升到可容纳的最外层）
        for (var d = $from.depth; d > 0; d--) {
          if ($from.node(d).type.name === "blockquote") {
            return lift(state, dispatch);
          }
        }
        // 列表项：光标块是 list_item 的直接子块 → 脱离列表变独立段落
        if ($from.depth >= 2 && $from.node($from.depth - 1).type.name === "list_item") {
          return lift(state, dispatch);
        }
        // 代码块内容起点 → 转正文段落（内容保留）
        if ($from.parent.type.name === "code_block") {
          return setBlockType(state.schema.nodes.paragraph)(state, dispatch);
        }
        return false;
      };
    }
  }
});

var mdHintKey = new PluginKey("hancic-md-hint");

var mdHint = $prose(function () {
  return new Plugin({
    key: mdHintKey,
    // ProseMirror 的 decorations 必须经 props 暴露（仅 state 不会应用到视图）
    props: {
      decorations: function (state) { return mdHintKey.getState(state); }
    },
    state: {
      init: function () { return DecorationSet.empty; },
      apply: function (tr, old, _oldState, _newState) {
        var sel = tr.selection;
        if (!sel || sel.$from === undefined) return old;
        var doc = tr.doc;
        var $from = sel.$from;
        if (!$from.parent || !$from.parent.isBlock) return old;
        if ($from.depth < 1) return old; // 空文档/顶层光标：无块级标记可显示
        var depth = $from.depth;
        var blockPos = $from.before(depth);
        var blockNode = $from.node(depth);
        var decos = [];
        var cls = [];
        // 当前段落高亮
        decos.push(Decoration.node(blockPos, blockPos + blockNode.nodeSize, { class: "hancic-md-active" }));
        // 块级标记：列表项/引用内是 <p>，标记归属其 list_item / blockquote 祖先
        var markerBlock = blockNode;
        var markerPos = blockPos;
        if (blockNode.type.name === "paragraph" && depth > 0) {
          var parent = $from.node(depth - 1);
          if (parent.type.name === "list_item" || parent.type.name === "blockquote") {
            markerBlock = parent;
            markerPos = $from.before(depth - 1);
          }
        }
        var marker = "";
        if (markerBlock.type.name === "heading") {
          marker = "#".repeat(markerBlock.attrs.level || 1) + " ";
        } else if (markerBlock.type.name === "blockquote") {
          marker = "> ";
        } else if (markerBlock.type.name === "code_block") {
          // 代码块标记不走 widget：widget 插 textblock 内容起点会阻断输入，
          // 语言标记由 CSS `pre::before` 常驻显示（不随光标）。
        } else if (markerBlock.type.name === "list_item") {
          var todo = markerBlock.attrs.todo;
          if (todo !== undefined) {
            marker = todo ? "- [x] " : "- [ ] ";
          } else {
            var list = markerPos === blockPos ? (depth > 0 ? $from.node(depth - 1) : null)
                                              : $from.node(depth - 2);
            if (list && list.type.name === "ordered_list") {
              var idx = 0;
              list.content.forEach(function (n, _o, i) {
                if (n === markerBlock) idx = i;
              });
              marker = (idx + 1) + ". ";
            } else {
              marker = "- ";
            }
          }
        }
        if (marker) {
          // 列表项：隐藏标记（- / 1. / - [ ]）与浏览器原生序号不同时显示——
          // 光标行关闭原生 list-style，仅显示隐藏标记；其余行保留原生序号。
          if (markerBlock.type.name === "list_item") {
            decos.push(Decoration.node(markerPos, markerPos + markerBlock.nodeSize, { class: "hancic-md-no-marker" }));
          }
          // widget 插到标记块**内容**的起点：跳过块标签（+1）与首个 block 子节点
          // （如 li > p / blockquote > p，再 +1），确保与文本同行；
          // 直接插 markerPos 会落到块元素外（如 <ol> 下 li 前）触发非法子元素换行。
          var widgetPos = markerPos + 1;
          var firstChild = markerBlock.childCount ? markerBlock.firstChild : null;
          if (firstChild && firstChild.isBlock) widgetPos = markerPos + 2;
          // 光标位于块首（offset 0）时标记激活，提示光标“经过”标记区
          var markerActive = $from.parentOffset === 0;
          decos.push(Decoration.widget(widgetPos, makeMdWidget(marker, widgetPos, markerActive)));
        }
        // 行内标记：光标所在 text node
        var hint = textNodeHint(blockNode, $from.parentOffset);
        if (hint) {
          var base = blockPos + 1;
          decos.push(Decoration.widget(base + hint.pos, makeMdWidget(hint.before, base + hint.pos)));
          decos.push(Decoration.widget(base + hint.pos + hint.len, makeMdWidget(hint.after, base + hint.pos + hint.len)));
        }
        return DecorationSet.create(doc, decos);
      }
    }
  });
});

// ---- mermaid 代码块：源码可编辑（默认 pre>code 视图），块下方实时渲染图预览 ----
// 用 decorations 的 widget 在代码块节点后插入预览区——**不替换 code_block 视图**。
// （此前尝试 props.nodeViews 覆盖 code_block，会导致代码块内 keydown 事件不触发
// 自定义 keymap，块首 Backspace 删标记失效；decoration widget 不影响光标与键盘。）
// widget 不占文档位置；每次文档变化重建，内部用渲染序号防竞态。
// mermaid.js 由页面另行引入（vendor/mermaid）。
var mermaidSeq = 0;
function mermaidWidgetFor(node) {
  return function () {
    var wrap = document.createElement("div");
    wrap.className = "hancic-mermaid-preview";
    wrap.innerHTML = '<span class="hancic-mermaid-hint">mermaid 预览（编辑自动刷新）</span>';
    var text = (node.textContent || "").trim();
    var seq = ++mermaidSeq;
    setTimeout(function () {
      if (!window.mermaid || !mermaid.render) {
        wrap.innerHTML = '<span class="hancic-mermaid-hint">mermaid 渲染库未加载</span>';
        return;
      }
      try {
        mermaid.initialize({ startOnLoad: false, theme: "default" });
        var pid = "hancic-mmd-" + Math.floor(Math.random() * 1e9) + "-" + seq;
        mermaid.render(pid, text).then(function (res) {
          if (wrap.isConnected) wrap.innerHTML = res.svg;
        }).catch(function () {
          if (wrap.isConnected) {
            wrap.innerHTML = '<span class="hancic-mermaid-hint hancic-mermaid-error">mermaid 渲染失败，请检查语法</span>';
          }
        });
      } catch (e) {
        if (wrap.isConnected) {
          wrap.innerHTML = '<span class="hancic-mermaid-hint hancic-mermaid-error">mermaid 渲染失败，请检查语法</span>';
        }
      }
    }, 0);
    return wrap;
  };
}

var mermaidPreview = $prose(function () {
  return new Plugin({
    props: {
      decorations: function (state) {
        var decos = [];
        state.doc.descendants(function (node, pos) {
          if (node.type.name === "code_block" &&
              String(node.attrs.language || node.attrs.lang || "").toLowerCase() === "mermaid" &&
              (node.textContent || "").trim()) {
            decos.push(Decoration.widget(pos + node.nodeSize, mermaidWidgetFor(node)));
          }
        });
        return DecorationSet.create(state.doc, decos);
      }
    }
  });
});

window.HancicEditor = {
  create: async function (opts) {
    var el = opts.el;
    var latestMd = opts.content || "";
    var savingDraft = false;

    var editor = await Editor.make()
      .config(configurePrism)
      .config(function (ctx) {
        ctx.set(rootCtx, el);
        ctx.set(defaultValueCtx, latestMd);
        ctx.get(listenerCtx).markdownUpdated(function (_ctx, md) {
          latestMd = md;
          if (opts.onUpdate) opts.onUpdate(md);
          // 新建页本地草稿（无服务端 autosave）
          if (opts.isNewPost && !savingDraft) {
            savingDraft = true;
            try {
              localStorage.setItem(opts.draftKey || "hancic-draft", md);
            } catch (e) { /* 忽略 */ }
            savingDraft = false;
          }
        });
      })
      .use(commonmark)
      .use(gfm)
      .use(mdHint)
      .use(exitCodeBlock)
      .use(delBlockMarker)
      .use(mermaidPreview)
      .use(prism)
      .use(history)
      .use(clipboard)
      .use(listener)
      .create();
    activeEditor = editor;

    // ---- 插入助手（全部通过 editor.action 拿到 ProseMirror view）----
    function insertImage(att) {
      editor.action(function (ctx) {
        var view = ctx.get(editorViewCtx);
        var node = view.state.schema.nodes.image.create(
          { src: "/uploads/" + att.path, alt: att.orig_name || "image" },
          null
        );
        view.dispatch(view.state.tr.replaceSelectionWith(node));
      });
    }

    function insertLink(text, href) {
      editor.action(function (ctx) {
        var view = ctx.get(editorViewCtx);
        var schema = view.state.schema;
        var mark = schema.marks.link.create({ href: href });
        var node = schema.text(text || href, [mark]);
        view.dispatch(view.state.tr.replaceSelectionWith(node));
      });
    }

    // 重新解析 Markdown 为文档；空内容用 "\n" 保证至少一个空段落（空 doc.content 替换会损坏文档）
    function setContent(md) {
      editor.action(function (ctx) {
        var view = ctx.get(editorViewCtx);
        var parser = ctx.get(parserCtx);
        var content;
        if (md && md.trim()) {
          content = parser(md).content;
        } else {
          // 空内容：直接构造空段落（parser 对空串/换行的解析结果不可靠）
          content = Fragment.from(view.state.schema.nodes.paragraph.create());
        }
        view.dispatch(view.state.tr.replaceWith(0, view.state.doc.content.size, content));
        latestMd = md || "";
      });
    }

    // ---- 图片粘贴 / 拖拽上传 ----
    // 在容器上以捕获阶段拦截 paste/drop，提取图片文件交给 opts.onUpload，
    // 上传成功后插入光标处；非图片文件不拦截。
    function isImage(f) {
      return f && typeof f.type === "string" && f.type.indexOf("image/") === 0;
    }

    function handleFiles(files) {
      var images = files.filter(isImage);
      if (!images.length) return false;
      Promise.resolve(opts.onUpload(images)).then(function (results) {
        (results || []).forEach(insertImage);
      }).catch(function () { /* 上传失败提示由 onUpload 内部处理 */ });
      return true;
    }

    function onPaste(e) {
      var files = e.clipboardData && e.clipboardData.files;
      if (files && files.length && handleFiles(Array.prototype.slice.call(files))) {
        e.preventDefault();
        e.stopPropagation();
      }
    }
    function onDrop(e) {
      var files = e.dataTransfer && e.dataTransfer.files;
      if (files && files.length && handleFiles(Array.prototype.slice.call(files))) {
        e.preventDefault();
        e.stopPropagation();
      }
    }
    el.addEventListener("paste", onPaste, true);
    el.addEventListener("drop", onDrop, true);

    // 强制序列化当前文档并返回最新 Markdown（提交前调用，规避 listener 异步延迟）
    var getMarkdownAsync = async function () {
      await editor.action(async function (ctx) {
        var view = ctx.get(editorViewCtx);
        var serializer = ctx.get(serializerCtx);
        latestMd = await serializer(view.state.doc);
      });
      return latestMd;
    };

    // 工具栏命令：直接用 ProseMirror 原生命令（setBlockType/toggleMark/wrapIn），
    // 不依赖 milkdown commandsCtx（其 call 需 $command 实例，直接执行需内部 ctx）
    function runCmd(fn) {
      return editor.action(function (ctx) {
        var view = ctx.get(editorViewCtx);
        var schema = view.state.schema;
        fn(view.state, view.dispatch, schema);
        view.focus();
      });
    }
    var commandApi = {
      paragraph: function () {
        return runCmd(function (state, dispatch, schema) {
          return setBlockType(schema.nodes.paragraph)(state, dispatch);
        });
      },
      heading: function (level) {
        return runCmd(function (state, dispatch, schema) {
          return setBlockType(schema.nodes.heading, { level: level })(state, dispatch);
        });
      },
      strong: function () {
        return runCmd(function (state, dispatch, schema) { return toggleMark(schema.marks.strong)(state, dispatch); });
      },
      emphasis: function () {
        return runCmd(function (state, dispatch, schema) { return toggleMark(schema.marks.em)(state, dispatch); });
      },
      inlineCode: function () {
        return runCmd(function (state, dispatch, schema) { return toggleMark(schema.marks.inline_code)(state, dispatch); });
      },
      blockquote: function () {
        return runCmd(function (state, dispatch, schema) {
          // 引用不允许嵌套：光标已在引用内 → lift 解除引用；否则包裹为引用
          var $from = state.selection.$from;
          for (var d = $from.depth; d > 0; d--) {
            if ($from.node(d).type.name === "blockquote") {
              return lift(state, dispatch);
            }
          }
          return wrapIn(schema.nodes.blockquote)(state, dispatch);
        });
      },
      bulletList: function () {
        return runCmd(function (state, dispatch, schema) { return wrapIn(schema.nodes.bullet_list)(state, dispatch); });
      },
      orderedList: function () {
        return runCmd(function (state, dispatch, schema) { return wrapIn(schema.nodes.ordered_list)(state, dispatch); });
      },
      hr: function () {
        return runCmd(function (state, dispatch, schema) {
          if (!state.selection.empty) return false;
          dispatch(state.tr.replaceSelectionWith(schema.nodes.hr.create()).scrollIntoView());
          return true;
        });
      },
      // 插入 2×2 表格（GFM：表头行 + 数据行，单元格为段落）
      table: function () {
        return runCmd(function (state, dispatch, schema) {
          var nodes = schema.nodes;
          if (!nodes.table || !state.selection.empty) return false;
          var mkCell = function (header) {
            var type = header ? nodes.table_header : nodes.table_cell;
            return type.create(null, schema.nodes.paragraph.create());
          };
          var headerRow = nodes.table_header_row.create(null, [
            mkCell(true), mkCell(true)
          ]);
          var row = nodes.table_row.create(null, [mkCell(false), mkCell(false)]);
          var table = nodes.table.create(null, [headerRow, row]);
          dispatch(state.tr.replaceSelectionWith(table).scrollIntoView());
          return true;
        });
      },
      // 代码块：光标已在代码块内 → 改语言；否则新建（语言可指定）
      codeBlock: function (lang) {
        return runCmd(function (state, dispatch, schema) {
          var attrs = lang ? { language: lang } : undefined;
          return setBlockType(schema.nodes.code_block, attrs)(state, dispatch);
        });
      }
    };
    var api = {
      getMarkdown: function () { return latestMd; },
      getMarkdownAsync: getMarkdownAsync,
      command: commandApi,
      insertImage: insertImage,
      insertLink: insertLink,
      setContent: setContent,
      destroy: function () {
        el.removeEventListener("paste", onPaste, true);
        el.removeEventListener("drop", onDrop, true);
        editor.destroy();
      }
    };
    // 调试/测试钩子：暴露当前编辑器实例（如 E2E 清空内容）
    window._hancicEditor = api;
    return api;
  }
};
