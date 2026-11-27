import { createStartHandler, defaultStreamHandler } from "@tanstack/react-start/server";
import { createServerEntry } from "@tanstack/react-start/server-entry";

import { localeRedirect } from "@/server/localeRedirect";
import { seoFile } from "@/server/seoFiles";

const startFetch = createStartHandler(defaultStreamHandler);

export default createServerEntry({
  fetch: (request) => seoFile(request) ?? localeRedirect(request) ?? startFetch(request),
});
