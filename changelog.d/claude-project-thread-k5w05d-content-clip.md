### Fixed

- A partial frame skipped a node whose own content holds an anti-aliased clip that cuts the node's ink away entirely, although the clip's fractional edge shows that ink in a pixel the two share; moving such a clip did not damage that pixel either. The node's bound now keeps the part of the clip within one device pixel of the content, measured through the transforms inside the node's content. Under a transform above the node that scales down, the cull can still miss it ([gap](/docs/gaps.md#a-clip-in-a-nodes-own-content-under-an-ancestors-scale-below-1)).
