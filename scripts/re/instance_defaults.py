#!/usr/bin/env python3
"""The defaults of new instances: for every instance type of the kit's tile.ted files, the most common attribute order, the most common
value of each attribute, the wrappers it holds (points, children) and its points' most common attributes. Writes a JSON summary that
`instance_templates.py` turns into crates/wce_tile/src/instance_templates.rs.

    python3 -I scripts/re/instance_defaults.py <kit raw_data/terrain/tiles> <out.json>
"""
import collections, json, os, re, sys

TYPES = {
    "ProjectedDecalInstance", "VEGETATION_INSTANCE", "POINT_LIGHT_INSTANCE", "VFXInstance", "SIMPLE_MESH_INSTANCE", "AI_HINT_POLYGON_INSTANCE",
    "NOGO_REGION_POLYGON_INSTANCE", "GO_REGION_POLYGON_INSTANCE", "SIEGE_AI_POLYGON_INSTANCE", "VegetationExclusionPolygonInstance",
    "ZONE_POLYGON_INSTANCE", "DeploymentZonePolygonInstanceGroup", "DEPLOYMENT_ZONE_POLYGON_INSTANCE", "CaptureLocationInstance",
    "CaptureLocationPolygonInstance", "CivilianDeploymentZone", "CivilianShelter", "LiteBuildingZone", "SiegeAINodeInstance",
    "SiegeAIEdgeInstance", "AISeparatorInstance", "SplineRiverInstance", "SEGMENTED_SPLINE_INSTANCE", "BUILDING_INSTANCE",
}
TAG = re.compile(rb'^\s*<(/?)([A-Za-z_][A-Za-z_0-9]*)((?:\s+[A-Za-z_][A-Za-z_0-9]*="[^"]*")*)\s*(/?)>\s*$')
ATTR = re.compile(rb'([A-Za-z_][A-Za-z_0-9]*)="([^"]*)"')

orders = collections.defaultdict(collections.Counter)
values = collections.defaultdict(lambda: collections.defaultdict(collections.Counter))
shapes = collections.defaultdict(collections.Counter)
point_attrs = collections.defaultdict(collections.Counter)
point_values = collections.defaultdict(lambda: collections.defaultdict(collections.Counter))
counts = collections.Counter()

root, out = sys.argv[1], sys.argv[2]
files = [os.path.join(d, f) for d, _, fs in os.walk(root) for f in fs if f == "tile.ted"]
for n, path in enumerate(sorted(files)):
    if n % 100 == 0:
        print(f"{n}/{len(files)}", file=sys.stderr)
    # The stack: [name, direct child names, owning instance type of a points wrapper]
    stack = []
    with open(path, "rb") as fh:
        for line in fh:
            m = TAG.match(line)
            if not m:
                continue
            close, name, attrs, leaf = m.group(1), m.group(2).decode(), m.group(3), m.group(4)
            if close:
                el = stack.pop()
                if el[0] in TYPES:
                    shapes[el[0]][tuple(el[1])] += 1
                continue
            if stack:
                stack[-1][1].append(name)
            if name in TYPES:
                counts[name] += 1
                pairs = [(k.decode(), v.decode()) for k, v in ATTR.findall(attrs)]
                orders[name][tuple(k for k, _ in pairs)] += 1
                for k, v in pairs:
                    values[name][k][v] += 1
            if name == "point" and len(stack) >= 2 and stack[-1][0] == "points" and stack[-2][0] in TYPES:
                owner = stack[-2][0]
                pairs = [(k.decode(), v.decode()) for k, v in ATTR.findall(attrs)]
                point_attrs[owner][tuple(k for k, _ in pairs)] += 1
                for k, v in pairs:
                    point_values[owner][k][v] += 1
            if leaf:
                if name in TYPES:
                    shapes[name][()] += 1
            else:
                stack.append([name, []])

summary = {}
for t in sorted(counts):
    summary[t] = {
        "count": counts[t],
        "order": list(orders[t].most_common(1)[0][0]),
        "values": {k: c.most_common(1)[0][0] for k, c in values[t].items()},
        "shape": list(shapes[t].most_common(1)[0][0]) if shapes[t] else [],
        "point_order": list(point_attrs[t].most_common(1)[0][0]) if point_attrs[t] else [],
        "point_values": {k: c.most_common(1)[0][0] for k, c in point_values[t].items()},
    }
json.dump(summary, open(out, "w"), indent=1)
print("done", file=sys.stderr)
