import { Component, type ReactNode, type ErrorInfo } from "react";

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
  name?: string;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export default class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const name = this.props.name ?? "Component";
    console.error(`[ErrorBoundary] ${name} crashed:`, error, info.componentStack);
  }

  render() {
    if (this.state.hasError) {
      if (this.props.fallback !== undefined) {
        return this.props.fallback;
      }
      return (
        <div
          style={{
            padding: "12px 16px",
            background: "rgba(255, 80, 80, 0.08)",
            border: "1px solid rgba(255, 80, 80, 0.2)",
            borderRadius: 8,
            color: "var(--text-tertiary)",
            fontSize: "var(--font-base)",
          }}
        >
          {this.props.name ?? "Component"} unavailable
        </div>
      );
    }
    return this.props.children;
  }
}
