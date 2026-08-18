import { useEffect, useRef } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";

export type PreviewObject = "sphere" | "cube" | "plane" | "cylinder";

/** Slot → image data URL (base-color, roughness, metallic, normal, ao, height). */
export type PreviewMaps = Partial<Record<string, string>>;

function makeGeometry(object: PreviewObject): THREE.BufferGeometry {
  switch (object) {
    case "cube":
      return new THREE.BoxGeometry(1.4, 1.4, 1.4);
    case "plane":
      return new THREE.PlaneGeometry(2, 2, 1, 1);
    case "cylinder":
      return new THREE.CylinderGeometry(0.85, 0.85, 1.7, 64, 1);
    case "sphere":
    default:
      return new THREE.SphereGeometry(1, 64, 48);
  }
}

// Live PBR material preview (spec §14). Builds a MeshStandardMaterial from the
// material's map thumbnails, lit by a generated studio (RoomEnvironment) so no
// HDRI asset is required, with orbit controls, exposure, and background toggle.
export function MaterialPreview({
  maps,
  signature,
  object = "sphere",
  background = false,
  exposure = 1,
  autoRotate = true,
  className,
}: {
  maps: PreviewMaps;
  /** Rebuild key — change it when the maps or material change. */
  signature: string;
  object?: PreviewObject;
  background?: boolean;
  exposure?: number;
  autoRotate?: boolean;
  className?: string;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const stateRef = useRef<{
    renderer?: THREE.WebGLRenderer;
    scene?: THREE.Scene;
    env?: THREE.Texture;
    autoRotate?: boolean;
  }>({});

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const width = container.clientWidth || 300;
    const height = container.clientHeight || 300;

    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    } catch {
      container.innerHTML =
        '<div style="display:flex;height:100%;align-items:center;justify-content:center;color:#7b8794;font-size:12px">WebGL unavailable</div>';
      return;
    }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.setSize(width, height);
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = exposure;
    container.appendChild(renderer.domElement);

    const scene = new THREE.Scene();
    const pmrem = new THREE.PMREMGenerator(renderer);
    const env = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;
    scene.environment = env;
    scene.background = background ? env : null;

    const camera = new THREE.PerspectiveCamera(45, width / height, 0.1, 100);
    camera.position.set(0, 0, 3.2);

    const geometry = makeGeometry(object);
    const uv = geometry.getAttribute("uv");
    if (uv) geometry.setAttribute("uv2", uv); // enable aoMap

    const material = new THREE.MeshStandardMaterial({
      color: 0xffffff,
      roughness: 1,
      metalness: 0,
    });

    const loader = new THREE.TextureLoader();
    const loaded: THREE.Texture[] = [];
    const load = (url: string, srgb = false) => {
      const t = loader.load(url);
      t.colorSpace = srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
      t.anisotropy = 4;
      loaded.push(t);
      return t;
    };

    if (maps.base_color) material.map = load(maps.base_color, true);
    if (maps.roughness) {
      material.roughnessMap = load(maps.roughness);
      material.roughness = 1;
    }
    if (maps.metallic) {
      material.metalnessMap = load(maps.metallic);
      material.metalness = 1;
    }
    if (maps.normal) material.normalMap = load(maps.normal);
    if (maps.ao) material.aoMap = load(maps.ao);
    if (maps.height && !maps.normal) {
      material.bumpMap = load(maps.height);
      material.bumpScale = 0.03;
    }
    if (!maps.base_color && !maps.roughness && !maps.normal && !maps.metallic) {
      material.color.set(0x8a8f98); // neutral when no maps
    }
    material.needsUpdate = true;

    const mesh = new THREE.Mesh(geometry, material);
    scene.add(mesh);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.enablePan = false;
    controls.minDistance = 1.8;
    controls.maxDistance = 6;

    const st = stateRef.current;
    st.renderer = renderer;
    st.scene = scene;
    st.env = env;
    st.autoRotate = autoRotate;

    let raf = 0;
    const animate = () => {
      raf = requestAnimationFrame(animate);
      if (st.autoRotate) mesh.rotation.y += 0.005;
      controls.update();
      renderer.render(scene, camera);
    };
    animate();

    const ro = new ResizeObserver(() => {
      const w = container.clientWidth;
      const h = container.clientHeight;
      if (w && h) {
        renderer.setSize(w, h);
        camera.aspect = w / h;
        camera.updateProjectionMatrix();
      }
    });
    ro.observe(container);

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      controls.dispose();
      geometry.dispose();
      material.dispose();
      loaded.forEach((t) => t.dispose());
      env.dispose();
      pmrem.dispose();
      renderer.dispose();
      if (renderer.domElement.parentNode) {
        renderer.domElement.parentNode.removeChild(renderer.domElement);
      }
      stateRef.current = {};
    };
    // Rebuild only when the material/maps or geometry changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature, object]);

  // Live updates that don't require a rebuild.
  useEffect(() => {
    const r = stateRef.current.renderer;
    if (r) r.toneMappingExposure = exposure;
  }, [exposure]);
  useEffect(() => {
    const { scene, env } = stateRef.current;
    if (scene) scene.background = background ? env ?? null : null;
  }, [background]);
  useEffect(() => {
    stateRef.current.autoRotate = autoRotate;
  }, [autoRotate]);

  return <div ref={containerRef} className={className} />;
}
