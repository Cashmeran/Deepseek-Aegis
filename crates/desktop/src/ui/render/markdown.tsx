import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import rehypeRaw from "rehype-raw";
import remarkGfm from "remark-gfm";

function openExternal(href: string) {
  // Tauri v2 webview: open in system browser via shell plugin
  // Fallback: window.open with noopener (Tauri opens these externally)
  try {
    window.__TAURI__?.opener?.openUrl(href);
  } catch {
    window.open(href, "_blank", "noopener,noreferrer");
  }
}

export default function MDContent({ text }: { text: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      rehypePlugins={[rehypeRaw, rehypeHighlight]}
      components={{
        // Intercept links — open in system browser instead of navigating webview
        a: (props) => (
          <a
            style={{ color: "var(--fg-muted)", textDecoration: "underline", cursor: "pointer" }}
            href={props.href}
            onClick={(e) => {
              e.preventDefault();
              if (props.href) openExternal(props.href);
            }}
            title={props.href}
          >
            {props.children}
          </a>
        ),
        h1: (props) => <h1 className="md-h1" {...props} />,
        h2: (props) => <h2 className="md-h2" {...props} />,
        h3: (props) => <h3 className="md-h3" {...props} />,
        p: (props) => <p className="md-p" {...props} />,
        ul: (props) => <ul className="md-ul" {...props} />,
        ol: (props) => <ol className="md-ol" {...props} />,
        li: (props) => <li className="md-li" {...props} />,
        strong: (props) => <strong className="md-strong" {...props} />,
        em: (props) => <em className="md-em" {...props} />,
        pre: (props) => (
          <pre className="md-pre" {...props} />
        ),
        code: (props) => {
          const { children, className, ...rest } = props;
          const match = /language-(\w+)/.exec(className || "");
          const isInline = !match && !String(children).includes("\n");

          return isInline ? (
            <code className="md-code-inline" {...rest}>
              {children}
            </code>
          ) : (
            <code className={`${className} md-code-block`} {...rest}>
              {children}
            </code>
          );
        }
      }}
    >
      {String(text ?? "")}
    </ReactMarkdown>
  )
}
