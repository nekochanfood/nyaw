import { createStartHandler, defaultStreamHandler } from "@tanstack/react-start/server";
import { createServerEntry } from "@tanstack/react-start/server-entry";

import { localeRedirect } from "@/server/localeRedirect";

const startFetch = createStartHandler(defaultStreamHandler);

export default createServerEntry({
  fetch: (request) => localeRedirect(request) ?? startFetch(request),
});
