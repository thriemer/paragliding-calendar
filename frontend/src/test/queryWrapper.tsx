import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import React from "react";

export function makeWrapper(): {
  client: QueryClient;
  wrapper: ({ children }: { children: React.ReactNode }) => React.ReactElement;
} {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { client, wrapper };
}
