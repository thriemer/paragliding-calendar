# TravelAI

## Important:

Loading secrets from env

```bash
eval "$(./load_env.sh)"
```

# Paragliding Flight Log Analytics

## Per Flight Log Analytics

- [x] Flight Duration
- [x] Distance Takeoff - Landing
- [x] Height over Takeoff
- [x] Track Log Length
- [x] Maximum/Average Climb/Sink
- [x] Glide Ratio
- [x] Total Elevation gained
- [ ] Time spent thermaling, gliding, soaring/ridge running
- [ ] Wind Speed
- [ ] Thermal trigger points/thermal origin
- [ ] Task Analytics (FAI - Triangle, Flat Triangle, Out and Return)

## Combined Flight Analytics

- [ ] General statistics (#flights per country/wing, histogram of length/distance)
- [ ] Popular takeoffs and popular inofficial takeoffs
- [ ] Popular landing fields
- [ ] Thermal climb rate forecast
- [ ] Average glide,speed,distance per glider

## Data Sources

- XCContest
- DHV-XC
- XC Globe
    http://xcglobe.com/flights/igc/2749468
- Paragliding Forum
- CFD
- XC League
- French Paragliding Association's website
- flightlog.org
    - scraping kml files work, but the glider info needs to be pulled
