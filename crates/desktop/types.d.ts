type ClientEvent = import("./src/ui/types").ClientEvent;
type ServerEvent = import("./src/ui/types").ServerEvent;

interface Window {
    __TAURI__?: {
        core?: {
            invoke: <T = unknown>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
        };
        event?: {
            listen: <T = unknown>(
                event: string,
                handler: (event: { payload: T }) => void
            ) => Promise<() => void>;
        };
    };
}
