package types

// Point — точка на карте (сетка Шереметьево).
type Point struct {
	X int64 `json:"x"`
	Y int64 `json:"y"`
}

// Route — маршрут по сетке.
type Route []Point

// MapMatrix — матрица проходимости/весов (см. Rust MapMatrix).
type MapMatrix struct {
	Grid [][]int64 `json:"grid"`
}

// MapObject — объект на интерактивной карте
// (ТЗ: ВС, ангары, терминалы, спецтехника, инженеры, инфраструктура).
type MapObject struct {
	ID       string        `json:"id"`
	Kind     MapObjectKind `json:"kind"`
	Label    string        `json:"label"`
	Position Point         `json:"position"`
}

type MapObjectKind string

const (
	ObjAircraft       MapObjectKind = "aircraft"
	ObjHangar         MapObjectKind = "hangar"
	ObjTerminal       MapObjectKind = "terminal"
	ObjVehicle        MapObjectKind = "vehicle"
	ObjEngineer       MapObjectKind = "engineer"
	ObjInfrastructure MapObjectKind = "infrastructure"
)
