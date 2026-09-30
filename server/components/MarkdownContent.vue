<script lang="ts">
// Renders markdown as Vue nodes rather than as an HTML string through v-html,
// so nothing in the source can reach the DOM as markup. It follows micromark's
// safe defaults, which the pages used before: raw HTML in the source is shown
// as text, and link/image URLs are dropped unless their protocol is allowed.
import type { Nodes, Parents, PhrasingContent, RootContent } from "mdast";
import { fromMarkdown } from "mdast-util-from-markdown";
import { sanitizeUri } from "micromark-util-sanitize-uri";
import { h, type VNodeChild } from "vue";

// micromark's own allowlists for href and src.
const linkProtocols = /^(https?|ircs?|mailto|xmpp)$/i;
const imageProtocols = /^https?$/i;

type Definitions = Map<string, { url: string; title?: string | null }>;

function collectDefinitions(node: Nodes, into: Definitions) {
  if (node.type === "definition") {
    if (!into.has(node.identifier)) into.set(node.identifier, node);
    return;
  }
  if ("children" in node) {
    for (const child of node.children) collectDefinitions(child, into);
  }
}

// Blocks are separated by a line break, as in micromark's output; without it,
// adjacent blocks that render as bare text (raw HTML) would run together.
function blocks(
  parent: Parents,
  defs: Definitions,
  tight: boolean,
): VNodeChild[] {
  return (parent.children as RootContent[]).flatMap((child, i) =>
    i === 0 ? [render(child, defs, tight)] : ["\n", render(child, defs, tight)],
  );
}

function render(
  node: RootContent,
  defs: Definitions,
  tight: boolean,
): VNodeChild {
  const children = (parent: Parents, childTight = tight) =>
    (parent.children as RootContent[]).map((child) =>
      render(child, defs, childTight),
    );

  switch (node.type) {
    case "text":
      return node.value;
    case "html":
      return node.value;
    case "paragraph":
      // A tight list's items hold their text directly, as micromark's do.
      return tight ? children(node) : h("p", children(node));
    case "heading":
      return h(`h${node.depth}`, children(node));
    case "thematicBreak":
      return h("hr");
    case "blockquote":
      return h("blockquote", blocks(node, defs, false));
    case "list": {
      const itemsTight =
        !node.spread && !node.children.some((item) => item.spread);
      const items = node.children.map((item) =>
        h("li", blocks(item, defs, itemsTight)),
      );
      return node.ordered
        ? h("ol", { start: node.start === 1 ? undefined : node.start }, items)
        : h("ul", items);
    }
    case "code":
      return h("pre", [
        h(
          "code",
          { class: node.lang ? `language-${node.lang}` : undefined },
          node.value,
        ),
      ]);
    case "inlineCode":
      return h("code", node.value);
    case "emphasis":
      return h("em", children(node));
    case "strong":
      return h("strong", children(node));
    case "break":
      return h("br");
    case "link":
      return h(
        "a",
        {
          href: sanitizeUri(node.url, linkProtocols),
          title: node.title ?? undefined,
        },
        children(node),
      );
    case "image":
      return h("img", {
        src: sanitizeUri(node.url, imageProtocols),
        alt: node.alt ?? "",
        title: node.title ?? undefined,
      });
    case "linkReference": {
      const def = defs.get(node.identifier);
      if (!def) return children(node);
      return h(
        "a",
        {
          href: sanitizeUri(def.url, linkProtocols),
          title: def.title ?? undefined,
        },
        children(node),
      );
    }
    case "imageReference": {
      const def = defs.get(node.identifier);
      if (!def) return node.alt ?? "";
      return h("img", {
        src: sanitizeUri(def.url, imageProtocols),
        alt: node.alt ?? "",
        title: def.title ?? undefined,
      });
    }
    case "definition":
      return null;
    default:
      // Node types outside CommonMark (none are parsed without extensions):
      // keep their text rather than dropping it.
      return "children" in node
        ? (node.children as PhrasingContent[]).map((child) =>
            render(child, defs, tight),
          )
        : "value" in node
          ? String(node.value)
          : null;
  }
}

export default defineComponent({
  props: {
    source: { type: String, required: true },
  },
  setup(props) {
    const tree = computed(() => fromMarkdown(props.source));
    return () => {
      const defs: Definitions = new Map();
      collectDefinitions(tree.value, defs);
      return h("div", blocks(tree.value, defs, false));
    };
  },
});
</script>
